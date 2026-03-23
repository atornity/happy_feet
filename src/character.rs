use std::mem;

use avian3d::prelude::*;
use bevy::prelude::*;

use crate::{
    collide_and_slide::{
        CollideAndSlideConfig, CollideAndSlideFilter, CollisionResponse, MovementState, SlideInfo,
        add_collider_to_filter, collide_and_slide, init_filter_mask, remove_collider_from_filter,
    },
    grounding::{Ground, Grounding, GroundingConfig, GroundingState, is_walkable, walkable_angle},
    moving_platform::InheritedVelocity,
    projection::{CollisionState, Surface, align_with_surface},
    stepping::{StepOutput, SteppingBehaviour, SteppingConfig, perform_step},
    sweep::{SweepHitData, SweepInput},
};

#[derive(SystemSet, Debug, PartialEq, Eq, Hash, Clone, Copy)]
pub struct CharacterSystems;

// TODO: the name is misleading since it supports moving other things as well
pub struct CharacterPlugin;

impl Plugin for CharacterPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<CollideAndSlideConfig>();

        app.add_observer(init_filter_mask);
        app.add_observer(add_collider_to_filter);
        app.add_observer(remove_collider_from_filter::<Replace>);
        app.add_observer(remove_collider_from_filter::<Remove>);

        app.configure_sets(
            PhysicsSchedule,
            CharacterSystems.in_set(NarrowPhaseSystems::Last),
        );

        app.add_systems(PhysicsSchedule, process_movement.in_set(CharacterSystems));
    }
}

/// A component for setting up character movement with grounding and stepping behavior.
#[derive(Component, Reflect, Default, Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[reflect(Component, Default, Debug, Clone)]
#[require(
    RigidBody = RigidBody::Kinematic,
    KinematicVelocity,
    InheritedVelocity,
    GroundingConfig,
    SteppingConfig,
    CollisionEventsEnabled,
)]
pub struct Character;

/// The actual movement during the last [`collide-and-slide`](collide_and_slide) update.
#[derive(Component, Reflect, Deref, Debug, Default, Clone, Copy)]
#[reflect(Component, Debug, Default, Clone)]
#[component(immutable)]
pub struct MovementDelta(pub Vec3);

/// The velocity of a kinematic body that is moved using [`collide-and-slide`](collide_and_slide).
#[derive(Component, Reflect, Deref, DerefMut, Debug, Default, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[reflect(Component, Debug, Default, Clone)]
#[require(CollideAndSlideFilter)]
pub struct KinematicVelocity(pub Vec3);

impl KinematicVelocity {
    /// Speed up towards a target speed along a given direction.
    /// The acceleration is clamped to avoid overshooting the target speed.
    pub fn accelerate(
        &mut self,
        direction: Dir3,
        max_acceleration: f32,
        target_speed: f32,
        delta: f32,
    ) {
        let accel = crate::movement::acceleration(
            self.0,
            *direction,
            max_acceleration,
            target_speed,
            delta,
        );
        self.0 += accel;
    }
}

/// Event that is triggered when a character collides with an obstacle during movement.
#[derive(EntityEvent, Reflect)]
pub struct OnSlide {
    pub entity: Entity,
    /// The velocity of the entity at the moment of impact
    pub velocity: Vec3,
    /// The slide duration
    pub duration: f32,
    pub input: SweepInput,
    pub hit: SweepHitData,
    pub surface: Surface,
}

/// Triggered when a character stepped over an obstacle.
#[derive(EntityEvent, Reflect)]
pub struct OnStep {
    pub entity: Entity,
    /// The velocity of the entity before stepping
    pub velocity: Vec3,
    /// The translation of the character before stepping.
    pub origin: Vec3,
    /// The movement of the character during the step.
    pub offset: Vec3,
    pub hit: SweepHitData,
}

#[allow(clippy::type_complexity)]
fn process_movement(
    // mut gizmos: Gizmos,
    global_collide_and_slide_config: Res<CollideAndSlideConfig>,
    mut spatial_params: ParamSet<(SpatialQuery, Query<&mut Position>)>,
    mut commands: Commands,
    mut query: Query<(
        Entity,
        &mut KinematicVelocity,
        &Rotation,
        &Collider,
        &CollideAndSlideFilter,
        Option<&CollideAndSlideConfig>,
        Option<(&mut Grounding, &GroundingConfig, &mut GroundingState)>,
        Option<&SteppingConfig>,
        Has<CollisionEventsEnabled>,
        Has<Sensor>,
    )>,
    rigid_bodies: Query<&RigidBody>,
    colliders_of: Query<&ColliderOf>,
    sensors: Query<Entity, With<Sensor>>,
    time: Res<Time>,
    mut collision_started_events: MessageWriter<CollisionStart>,
    mut collision_ended_events: MessageWriter<CollisionEnd>,
) {
    for (
        entity,
        mut velocity,
        rotation,
        collider,
        filter,
        collide_and_slide_config,
        grounding,
        stepping,
        collision_events_enabled,
        is_sensor,
    ) in &mut query
    {
        let position_val = spatial_params.p1().get(entity).unwrap().0;

        // Sensors don't need to collide
        // TODO: should we still trigger OnHit events maybe?
        if is_sensor {
            spatial_params.p1().get_mut(entity).unwrap().0 =
                position_val + velocity.0 * time.delta_secs();
            continue;
        }

        let collide_and_slide_config = collide_and_slide_config
            .copied()
            .unwrap_or(*global_collide_and_slide_config);

        // Filter out sensor entities from collision detection
        let filter_hits = |hit: &SweepHitData| !sensors.contains(hit.entity);

        let is_grounded = grounding.as_ref().is_some_and(|(g, ..)| g.is_grounded());

        let mut collision = CollisionState::default();
        let mut movement = MovementState::new(velocity.0, position_val, time.delta_secs());
        let mut previous_velocity = movement.velocity;

        let mut slide_events = Vec::<OnSlide>::new();

        {
            let query_pipeline = spatial_params.p0();
            collide_and_slide(
                &mut movement,
                collider,
                rotation.0,
                &collide_and_slide_config,
                &query_pipeline,
                filter,
                |hit| {
                    if !filter_hits(hit) {
                        return None;
                    }

                    Some(match grounding.as_ref() {
                        Some((grounding, grounding_config, _)) => Surface::new(
                            hit.normal,
                            walkable_angle(grounding_config.max_angle, grounding.is_grounded()),
                            grounding_config.up_direction,
                        ),
                        None => Surface {
                            normal: Dir3::new(hit.normal).unwrap(),
                            is_walkable: false,
                        },
                    })
                },
                |movement,
                 SlideInfo {
                     input,
                     hit,
                     surface,
                 }| {
                    // Stepping logic
                    if !surface.is_walkable
                        && let Some((stepping_config, (grounding, grounding_config, _))) =
                            stepping.zip(grounding.as_ref())
                        && match stepping_config.behaviour {
                            SteppingBehaviour::Never => false,
                            SteppingBehaviour::Grounded => grounding.is_grounded(),
                            SteppingBehaviour::Always => true,
                        }
                    {
                        let remaining_horizontal_velocity = (movement.velocity
                            * movement.remaining_time)
                            .reject_from(*grounding_config.up_direction);

                        if let Ok((horizontal_direction, horizontal_motion)) =
                            Dir3::new_and_length(remaining_horizontal_velocity)
                            && let Some(StepOutput {
                                horizontal,
                                vertical,
                                hit: step_hit,
                            }) = perform_step(
                                stepping_config,
                                collider,
                                movement.position(),
                                rotation.0,
                                horizontal_direction,
                                horizontal_motion,
                                grounding_config.up_direction,
                                collide_and_slide_config.skin_width,
                                &query_pipeline,
                                filter,
                                filter_hits,
                                |hit| {
                                    // Only step on surfaces that are walkable
                                    if !is_walkable(
                                        hit.normal,
                                        stepping_config
                                            .max_angle
                                            .unwrap_or(grounding_config.max_angle),
                                        *grounding_config.up_direction,
                                    ) {
                                        return false;
                                    }

                                    // Stepping on dynamic bodies is a bit buggy right now ):
                                    if let Ok(rb) = rigid_bodies.get(hit.entity)
                                        && rb.is_dynamic()
                                    {
                                        return false;
                                    }

                                    true
                                },
                            )
                        {
                            let offset = grounding_config.up_direction * vertical
                                + horizontal_direction * horizontal;
                            let duration = horizontal * time.delta_secs();

                            // Trigger step event
                            commands.entity(entity).trigger(|entity| OnStep {
                                entity,
                                origin: movement.position(),
                                velocity: movement.velocity,
                                offset,
                                hit: step_hit,
                            });

                            // Update movement state
                            movement.offset += offset;
                            movement.velocity = align_with_surface(
                                movement.velocity,
                                step_hit.normal,
                                *grounding_config.up_direction,
                            );
                            movement.ground = Some(Ground::new(step_hit.entity, step_hit.normal));
                            movement.remaining_time = (movement.remaining_time - duration).max(0.0);

                            // Obstruction was avoided, skip projecting velocity
                            return CollisionResponse::Skip;
                        }
                    }

                    // Push slide event
                    if let Some(last) = slide_events.last_mut() {
                        last.duration -= movement.remaining_time;
                    }
                    slide_events.push(OnSlide {
                        entity: Entity::PLACEHOLDER,
                        velocity: movement.velocity,
                        duration: movement.remaining_time,
                        input,
                        hit,
                        surface,
                    });

                    // Write collision events
                    if collision_events_enabled {
                        collision_started_events.write(CollisionStart {
                            collider1: entity,
                            collider2: hit.entity,
                            body1: colliders_of.get(entity).ok().map(|of| of.body),
                            body2: colliders_of.get(hit.entity).ok().map(|of| of.body),
                        });
                        // Assume the collision is ended immediately, which it did because we slided (:
                        collision_ended_events.write(CollisionEnd {
                            collider1: entity,
                            collider2: hit.entity,
                            body1: colliders_of.get(entity).ok().map(|of| of.body),
                            body2: colliders_of.get(hit.entity).ok().map(|of| of.body),
                        });
                    }

                    CollisionResponse::Slide
                },
                |velocity, surface| match grounding.as_ref() {
                    Some((grounding, grounding_config, _))
                        if grounding_config.override_velocity_projection =>
                    {
                        collision.update(
                            surface,
                            velocity,
                            mem::replace(&mut previous_velocity, velocity),
                            is_grounded,
                            |vel| {
                                surface.project_velocity(
                                    vel,
                                    grounding.normal(),
                                    grounding_config.up_direction,
                                )
                            },
                        )
                    }
                    _ => velocity.reject_from(*surface.normal),
                },
            );
        }

        // Trigger slide events
        commands.queue(move |world: &mut World| {
            for mut event in slide_events {
                world.entity_mut(entity).trigger(|entity| {
                    event.entity = entity;
                    event
                });
            }
        });

        commands
            .entity(entity)
            .insert(MovementDelta(movement.offset));

        // Apply movement
        spatial_params.p1().get_mut(entity).unwrap().0 = position_val + movement.offset;
        velocity.0 = movement.velocity;

        if let Some((_, _, mut grounding_state)) = grounding {
            grounding_state.pending = movement.ground;
        }
    }
}
