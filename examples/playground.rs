//! This example is gross, truly awful, it's only used for testing and is not meant to be understandable.
//! See [minimal](minimal.rs) instead.

use std::f32::consts::PI;

use avian3d::prelude::*;
use bevy::{
    color::palettes::css::*,
    input::InputSystems,
    prelude::*,
    window::{CursorGrabMode, CursorOptions, PrimaryWindow},
};
use bevy_enhanced_input::prelude::{Press, *};
use bevy_skein::SkeinPlugin;
use happy_feet::{
    debug::{DebugGrounding, DebugInput},
    movement::jump,
    prelude::*,
};

fn main() -> AppExit {
    App::new()
        .add_plugins((
            DefaultPlugins,
            PhysicsPlugins::default(),
            SkeinPlugin::default(),
            CharacterPlugins::default(),
            EnhancedInputPlugin,
        ))
        .insert_resource(GlobalAmbientLight {
            color: ALICE_BLUE.into(),
            brightness: 200.0,
            ..Default::default()
        })
        .init_gizmo_group::<PhysicsGizmos>()
        .add_input_context::<Walking>()
        .add_observer(on_ground_enter)
        .add_observer(on_ground_leave)
        .add_observer(on_step)
        .add_observer(on_jump)
        .add_observer(on_toggle_perspective)
        .add_observer(on_toggle_fly_mode)
        .add_observer(on_toggle_no_clip)
        .add_systems(Startup, setup)
        .add_systems(PreUpdate, update_movement_settings)
        .add_systems(
            PreUpdate,
            (move_input, look_input).after(EnhancedInputSystems::Apply),
        )
        .add_systems(Update, (remove_ground_when_flying, capture_mouse))
        .add_systems(
            PostUpdate,
            (
                update_attachments.after(TransformSystems::Propagate),
                update_camera_offset,
                sync_attachment_global_transforms,
                update_camera_step_offset,
            )
                .chain(),
        )
        .add_systems(FixedUpdate, move_animated_platform)
        .run()
}

#[allow(dead_code)]
fn dbg_speed(query: Query<&KinematicVelocity>) {
    for velocity in query.iter() {
        dbg!("{:?}", velocity.length());
    }
}

fn capture_mouse(mut query: Query<&mut CursorOptions, Added<PrimaryWindow>>) {
    for mut window in &mut query {
        window.grab_mode = CursorGrabMode::Locked;
        window.visible = false;
    }
}

#[derive(Component, Reflect, Default, Debug)]
#[reflect(Component)]
struct PlayerCamera {
    eye_height: f32,
    distance: f32,
}

#[derive(Component, Reflect, Debug)]
#[reflect(Component)]
#[relationship_target(relationship = AttachedTo)]
struct Attachments(Vec<Entity>);

#[derive(Component, Reflect, Debug)]
#[reflect(Component)]
#[relationship(relationship_target = Attachments)]
#[require(AttachmentPosition)]
struct AttachedTo(Entity);

#[derive(Component, Reflect, Default, Debug)]
#[reflect(Component)]
struct AttachmentPosition(Vec3);

#[derive(Component, Reflect, Debug)]
#[reflect(Component)]
enum MovementMode {
    Walking,
    Flying,
}

impl MovementMode {
    fn settings(
        &self,
        grounded: bool,
    ) -> (
        CharacterMovement,
        CharacterDrag,
        CharacterGravity,
        GroundFriction,
    ) {
        let target_speed = 8.0;

        let ground_movement = CharacterMovement {
            target_speed,
            acceleration: 100.0,
        };

        let air_movement = CharacterMovement {
            target_speed,
            acceleration: 20.0,
        };

        match (self, grounded) {
            (MovementMode::Walking, true) => (
                ground_movement,
                CharacterDrag::default(),
                CharacterGravity(Some(Vec3::NEG_Y * 20.0)),
                GroundFriction::default(),
            ),
            (MovementMode::Walking, false) => (
                air_movement,
                CharacterDrag::default(),
                CharacterGravity(Some(Vec3::NEG_Y * 20.0)),
                GroundFriction::ZERO,
            ),
            (MovementMode::Flying, _) => (
                ground_movement,
                CharacterDrag(10.0),
                CharacterGravity::ZERO,
                GroundFriction::ZERO,
            ),
        }
    }
}

fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    asset_server: Res<AssetServer>,
) {
    let platform_size = Vec3::new(10.0, 0.4, 2.0);
    let platform_position = Vec3::new(10.0, 1.0, 10.0);
    let platform_offset = Vec3::new(4.0, 0.0, 0.0);
    let cylinder_height = 2.0;

    let platform_shape = Cuboid::from_size(platform_size);
    let cylinder_shape = Cylinder::new(0.5, cylinder_height);

    commands.spawn((
        RigidBody::Kinematic,
        AngularVelocity(Vec3::new(0.0, 1.0, 0.0)),
        Transform::from_translation(platform_position),
        CenterOfMass(platform_offset),
        Collider::from(platform_shape),
        Mesh3d(meshes.add(platform_shape)),
        MeshMaterial3d(materials.add(StandardMaterial::default())),
    ));
    commands.spawn((
        RigidBody::Static,
        Transform::from_translation(
            platform_position + platform_offset
                - Vec3::Y * (cylinder_height / 2.0 + platform_size.y / 2.0),
        ),
        Collider::from(cylinder_shape),
        Mesh3d(meshes.add(cylinder_shape)),
        MeshMaterial3d(materials.add(StandardMaterial::default())),
    ));

    let cube = Cuboid::new(4.0, 1.0, 4.0);
    commands.spawn((
        RigidBody::Dynamic,
        Transform::from_xyz(0.0, 2.0, 0.0),
        Collider::from(cube),
        Mesh3d(meshes.add(cube)),
        MeshMaterial3d(materials.add(StandardMaterial::default())),
        ConstantAngularAcceleration::new(0.0, 10.0, 0.0),
        ConstantLinearAcceleration::new(0.0, 0.0, 20.0),
        AngularDamping(10.0),
    ));

    // physics mover
    commands.spawn((
        AnimatedPlatform,
        PhysicsMover,
        RigidBody::Kinematic,
        Transform::from_xyz(-20.0, 1.0, 0.0),
        Collider::from(cube),
        Mesh3d(meshes.add(cube)),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: MIDNIGHT_BLUE.into(),
            ..Default::default()
        })),
    ));
    // make sure the physics mover moves at the same rate/distance as the normal platform
    commands.spawn((
        AnimatedPlatform,
        Transform::from_xyz(-24.0, 1.0, 0.0),
        Mesh3d(meshes.add(cube)),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: CRIMSON.with_alpha(0.5).into(),
            alpha_mode: AlphaMode::Blend,
            ..Default::default()
        })),
    ));

    let shape = Capsule3d::new(0.2, 1.0);
    // let shape = Cylinder::new(0.2, 1.0);

    commands.spawn((
        Name::new("Player"),
        MovementMode::Walking,
        (
            Character,
            DebugInput,
            DebugGrounding,
            CharacterMovement::default(),
            CharacterGravity::default(),
            GroundFriction::default(),
            CharacterDrag::default(),
            SteppingConfig {
                max_vertical: 0.4,
                behaviour: SteppingBehaviour::Always,
                ..Default::default()
            },
            GroundingConfig {
                max_angle: PI / 4.0 + 0.1,
                max_distance: 0.2,
                ..Default::default()
            },
            CollideAndSlideConfig {
                skin_width: 0.1,
                ..Default::default()
            },
        ),
        CameraStepOffset(Vec3::ZERO),
        walking_actions(),
        Walking,
        Mass(10.0),
        CollisionEventsEnabled,
        Collider::from(shape),
        Mesh3d(meshes.add(shape)),
        MeshMaterial3d(materials.add(StandardMaterial {
            alpha_mode: AlphaMode::Blend,
            base_color: WHITE.with_alpha(0.5).into(),
            ..Default::default()
        })),
        Transform {
            translation: Vec3::new(0.0, 2.0, -4.0),
            rotation: Quat::from_rotation_x(PI),
            ..Default::default()
        },
        Attachments::spawn_one((
            PlayerCamera {
                eye_height: 0.5,
                ..Default::default()
            },
            Projection::Perspective(PerspectiveProjection {
                fov: 75.0_f32.to_radians(),
                ..Default::default()
            }),
            Transform::from_xyz(0.0, 1.0, 10.0),
            Camera3d::default(),
        )),
    ));

    commands.spawn((SceneRoot(
        asset_server.load(GltfAssetLabel::Scene(0).from_asset("playground.gltf")),
    ),));
}

#[derive(Component, Reflect, Debug)]
#[reflect(Component)]
struct AnimatedPlatform;

fn move_animated_platform(
    mut query: Query<&mut Transform, With<AnimatedPlatform>>,
    time: Res<Time>,
) {
    for mut transform in &mut query {
        transform.translation.z = time.elapsed_secs().sin() * 10.0;
    }
}

#[derive(Component, Default)]
struct Walking;

#[derive(InputAction, Debug)]
#[action_output(Vec3)]
struct Move;

#[derive(InputAction, Debug)]
#[action_output(bool)]
struct Jump;

#[derive(InputAction, Debug)]
#[action_output(bool)]
struct Sneak;

#[derive(InputAction, Debug)]
#[action_output(Vec2)]
struct Look;

#[derive(InputAction, Debug)]
#[action_output(bool)]
struct TogglePerspective;

#[derive(InputAction, Debug)]
#[action_output(bool)]
struct ToggleFlyMode;

#[derive(InputAction, Debug)]
#[action_output(bool)]
struct ToggleNoClip;

fn walking_actions() -> impl Bundle {
    actions!(
        Walking[
            (
                Action::<Move>::new(),
                Bindings::spawn((
                    Cardinal::wasd_keys(),
                    Bidirectional {
                        positive: Binding::from(KeyCode::KeyE),
                        negative: Binding::from(KeyCode::KeyQ),
                    }.with(SwizzleAxis::YZX),
                )),
                Negate::y(),
                SwizzleAxis::XZY,
            ),
            (
                Action::<Jump>::new(),
                bindings![KeyCode::Space],
                Press::default(),
            ),
            (
                Action::<Sneak>::new(),
                bindings![KeyCode::ShiftLeft],
            ),
            (
                Action::<Look>::new(),
                bindings![
                    Binding::mouse_motion()
                ],
                Scale::splat(-0.01),
                SwizzleAxis::YXZ,
            ),
            (
                Action::<ToggleFlyMode>::new(),
                bindings![KeyCode::KeyF],
                Press::default(),
            ),
            (
                Action::<ToggleNoClip>::new(),
                bindings![KeyCode::Tab],
                Press::default(),
            ),
            (
                Action::<TogglePerspective>::new(),
                bindings![KeyCode::KeyC],
                Press::default(),
            )
        ]
    )
}

fn on_toggle_fly_mode(
    trigger: On<Fire<ToggleFlyMode>>,
    mut query: Query<(&mut MovementMode, &mut CharacterDrag, &mut CharacterGravity)>,
) {
    let (mut mode, mut drag, mut gravity) = query.get_mut(trigger.event().context).unwrap();
    match *mode {
        MovementMode::Walking => {
            *mode = MovementMode::Flying;

            drag.0 = 10.0;
            gravity.0 = Some(Vec3::ZERO);
        }
        MovementMode::Flying => {
            *mode = MovementMode::Walking;

            drag.0 = 0.01;
            gravity.0 = Some(Vec3::NEG_Y * 20.0);
        }
    }
}

fn on_toggle_no_clip(
    trigger: On<Fire<ToggleNoClip>>,
    mut commands: Commands,
    query: Query<Has<Sensor>>,
) {
    let is_sensor = query.get(trigger.event().context).unwrap();
    match is_sensor {
        true => commands.entity(trigger.event().context).remove::<Sensor>(),
        false => commands.entity(trigger.event().context).insert(Sensor),
    };
}

fn on_toggle_perspective(
    trigger: On<Fire<TogglePerspective>>,
    targets: Query<&Attachments>,
    mut cameras: Query<&mut PlayerCamera>,
) -> Result {
    let attachments = targets.get(trigger.event().context)?;

    let mut iter = cameras.iter_many_mut(attachments.iter());

    while let Some(mut player_camera) = iter.fetch_next() {
        match player_camera.distance > 0.0 {
            true => player_camera.distance = 0.0,
            false => player_camera.distance = 8.0,
        }
    }

    Ok(())
}

fn on_jump(
    trigger: On<Fire<Jump>>,
    mut query: Query<(
        &mut KinematicVelocity,
        &mut Grounding,
        &GroundingConfig,
        &MovementMode,
    )>,
) -> Result {
    let (mut velocity, mut grounding, grounding_config, mode) =
        query.get_mut(trigger.event().context)?;
    if let MovementMode::Walking = mode {
        jump(
            7.0,
            &mut velocity,
            &mut grounding,
            grounding_config.up_direction,
        );
    }
    Ok(())
}

fn update_movement_settings(
    mut query: Query<(
        &Grounding,
        &mut CharacterMovement,
        &mut CharacterDrag,
        &mut CharacterGravity,
        &mut GroundFriction,
        &MovementMode,
    )>,
) {
    for (grounding, mut movement, mut drag, mut gravity, mut friction, mode) in &mut query {
        let (new_movement, new_drag, new_gravity, new_friction) =
            mode.settings(grounding.is_grounded());
        *movement = new_movement;
        *drag = new_drag;
        *gravity = new_gravity;
        *friction = new_friction;
    }
}

fn remove_ground_when_flying(mut query: Query<(&mut Grounding, &MovementMode)>) {
    for (mut grounding, mode) in &mut query {
        if let MovementMode::Flying = mode {
            grounding.detach();
        }
    }
}

fn move_input(
    mut query: Query<(&mut MoveInput, &mut MovementMode)>,
    movement: Single<&Action<Move>>,
    sneak: Single<&Action<Sneak>>,
    camera: Single<&GlobalTransform, With<PlayerCamera>>,
) {
    let mut camera_transform = camera.compute_transform();

    for (mut input, mode) in &mut query {
        if let MovementMode::Walking = *mode {
            camera_transform.align(Dir3::Y, Dir3::Y, Dir3::NEG_Z, camera_transform.forward());
        }

        // info!("Movement: {}", movement.normalize_or_zero());
        input.set(camera_transform.rotation * movement.normalize_or_zero());

        if ***sneak {
            input.value *= 0.2;
        }
    }
}

fn look_input(
    look: Single<&Action<Look>>,
    mut cameras: Query<(&mut Transform, &Projection)>,
) -> Result {
    for (mut camera_transform, projection) in &mut cameras {
        let &Projection::Perspective(PerspectiveProjection { fov, .. }) = projection else {
            Err("expected perspective projection")?
        };

        let axis = ***look / fov;

        let (mut yaw, mut pitch, roll) = camera_transform.rotation.to_euler(EulerRot::YXZ);

        let max_pitch = PI / 2.0 - 1e-4;
        pitch = (pitch + axis.x).clamp(-max_pitch, max_pitch);
        yaw += axis.y;

        let rotation = Quat::from_euler(EulerRot::YXZ, yaw, pitch, roll);

        camera_transform.rotation = rotation;
    }

    Ok(())
}

fn update_camera_step_offset(mut query: Query<&mut CameraStepOffset>, time: Res<Time>) {
    for mut offset in &mut query {
        offset.0.smooth_nudge(&Vec3::ZERO, 20.0, time.delta_secs());
    }
}

fn update_camera_offset(
    characters: Query<(&GroundingConfig, &CameraStepOffset)>,
    mut cameras: Query<(&mut Transform, &PlayerCamera, &AttachedTo)>,
) -> Result {
    for (mut camera_transform, player_camera, attached_to) in &mut cameras {
        let (grounding_config, camera_step_offset) = characters.get(attached_to.0)?;

        let mut offset =
            camera_step_offset.0 + grounding_config.up_direction * player_camera.eye_height;
        offset += camera_transform.rotation * Vec3::Z * player_camera.distance;

        camera_transform.translation += offset;
    }

    Ok(())
}

fn update_attachments(
    mut query: Query<(&mut Transform, &AttachedTo)>,
    targets: Query<&GlobalTransform, Without<AttachedTo>>,
) -> Result {
    for (mut transform, attached_to) in &mut query {
        let target_transform = targets.get(attached_to.0)?;
        transform.translation = target_transform.translation();
    }

    Ok(())
}

fn sync_attachment_global_transforms(
    transform_helper: TransformHelper,
    mut query: Query<(Entity, &mut GlobalTransform), With<AttachedTo>>,
) -> Result {
    for (entity, mut transform) in &mut query {
        *transform = transform_helper.compute_global_transform(entity)?;
    }

    Ok(())
}

#[derive(Component, Reflect, Debug)]
#[reflect(Component)]
struct CameraStepOffset(Vec3);

fn on_ground_enter(_: On<OnGroundEnter>) {
    info!("ENTERED GROUND");
}

fn on_ground_leave(_: On<OnGroundLeave>) {
    info!("LEFT GROUND");
}

fn on_step(_: On<OnStep>) {
    info!("STEPPED");
}
