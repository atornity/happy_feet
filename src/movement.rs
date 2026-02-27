use std::mem;

use avian3d::prelude::*;
use bevy::{input::InputSystems, prelude::*};

use crate::{
    character::{KinematicVelocity, OnSlide},
    grounding::{Grounding, GroundingConfig},
    projection::align_with_surface,
};

pub struct MovementPlugin;

impl Plugin for MovementPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(PreUpdate, clear_movement_input.before(InputSystems));

        app.add_systems(
            PhysicsSchedule,
            (
                character_drag,
                character_friction,
                character_gravity,
                character_acceleration,
            )
                .in_set(PhysicsStepSystems::First)
                .chain(),
        );

        app.add_observer(bounce_on_character_hit);
    }
}

pub(crate) fn clear_movement_input(mut query: Query<&mut MoveInput>) {
    for mut move_input in &mut query {
        move_input.update();
    }
}

pub(crate) fn character_gravity(
    default_gravity: Res<Gravity>,
    mut query: Query<(
        &mut KinematicVelocity,
        &CharacterGravity,
        Option<&Grounding>,
        Option<&GravityScale>,
    )>,
    time: Res<Time>,
) {
    for (mut velocity, character_gravity, grounding, gravity_scale) in &mut query {
        if grounding.is_some_and(|g| g.is_grounded()) {
            continue;
        }

        let mut gravity = match character_gravity.0 {
            Some(gravity) => gravity,
            None => default_gravity.0,
        };

        if let Some(gravity_scale) = gravity_scale {
            gravity *= gravity_scale.0;
        }

        velocity.0 += gravity * time.delta_secs();
    }
}

pub(crate) fn character_friction(
    mut characters: Query<(&mut KinematicVelocity, &Grounding, &GroundFriction)>,
    frictions: Query<&FrictionScale>,
    colliders: Query<&ColliderOf>,
    time: Res<Time>,
) {
    for (mut velocity, grounding, character_friction) in &mut characters {
        let Some(ground) = grounding.ground() else {
            continue;
        };

        let mut friction = character_friction.0;

        // Multiply friction by the friction scale
        if let Ok(s) = frictions.get(ground.entity) {
            friction *= s.0;
        } else if let Ok(collider_of) = colliders.get(ground.entity)
            && let Ok(s) = frictions.get(collider_of.body)
        {
            friction *= s.0;
        }

        let f = friction_factor(velocity.0, friction, time.delta_secs());

        velocity.0 *= f;
    }
}

pub(crate) fn character_drag(
    mut query: Query<(&mut KinematicVelocity, &CharacterDrag)>,
    time: Res<Time>,
) {
    for (mut velocity, drag) in &mut query {
        velocity.0 *= drag_factor(drag.0, time.delta_secs());
    }
}

pub(crate) fn character_acceleration(
    mut query: Query<(
        &MoveInput,
        &mut KinematicVelocity,
        Option<(&Grounding, &GroundingConfig)>,
        &CharacterMovement,
        Option<&BrakeFactor>,
    )>,
    time: Res<Time>,
) {
    for (move_input, mut character_velocity, grounding, movement, brake_factor) in &mut query {
        let Ok((direction, throttle)) = Dir3::new_and_length(move_input.value) else {
            continue;
        };

        let mut direction = *direction;
        let mut velocity = character_velocity.0;

        if let Some((grounding, grounding_config)) = grounding {
            if let Some(normal) = grounding.normal() {
                direction = align_with_surface(direction, *normal, *grounding_config.up_direction);
                velocity = align_with_surface(velocity, *normal, *grounding_config.up_direction);
            }

            if brake_factor.is_some() {
                velocity = velocity.reject_from(*grounding_config.up_direction);
            }
        }

        let max_acceleration = movement.acceleration * throttle;

        let move_accel = match brake_factor {
            None => acceleration(
                velocity,
                direction,
                max_acceleration,
                movement.target_speed * throttle,
                time.delta_secs(),
            ),
            Some(brake_factor) => acceleration_with_brake(
                velocity,
                direction,
                max_acceleration,
                movement.target_speed * throttle,
                *brake_factor,
                time.delta_secs(),
            ),
        };

        character_velocity.0 += move_accel;
    }
}

pub(crate) fn bounce_on_character_hit(
    trigger: On<OnSlide>,
    mut query: Query<(
        &mut KinematicVelocity,
        &CharacterBounce,
        Option<&mut Grounding>,
    )>,
) {
    let Ok((mut velocity, bounce, grounding)) = query.get_mut(trigger.event().entity) else {
        return;
    };

    match bounce.behaviour {
        BounceBehaviour::Never => return,
        BounceBehaviour::Ground if !trigger.surface.is_walkable => return,
        BounceBehaviour::Obstruction if trigger.surface.is_walkable => return,
        _ => {}
    }

    let bounce_impulse = -trigger.velocity.dot(*trigger.surface.normal) * bounce.restitution;

    velocity.0 += *trigger.surface.normal * bounce_impulse;

    if bounce_impulse > 0.1
        && let Some(mut grounding) = grounding
    {
        grounding.detach();
    }
}

/// Used for modifying [`KinematicVelocity`] based on [`MoveInput`].
#[derive(Component, Reflect, Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[reflect(Component, Debug, Clone)]
#[require(MoveInput, KinematicVelocity)]
pub struct CharacterMovement {
    pub target_speed: f32,
    pub acceleration: f32,
}

impl Default for CharacterMovement {
    fn default() -> Self {
        Self {
            target_speed: 8.0,
            acceleration: 100.0,
        }
    }
}

/// The gravity force affecting a character while it's not grounded.
/// If no gravity is defined, then the [`Gravity`] resource will be used.
#[derive(Component, Reflect, Deref, DerefMut, Default, Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[reflect(Component, Default, Debug, Clone)]
#[require(KinematicVelocity)]
pub struct CharacterGravity(pub Option<Vec3>);

impl CharacterGravity {
    pub fn new(gravity: Vec3) -> Self {
        Self(Some(gravity))
    }
}

impl CharacterGravity {
    pub const ZERO: Self = Self(Some(Vec3::ZERO));
    pub const EARTH: Self = Self(Some(Vec3::new(0.0, -9.81, 0.0)));
}

/// The friction scale when a character walks on an entity.
#[derive(Component, Reflect, Deref, DerefMut, Default, Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[reflect(Component, Default, Debug, Clone)]
pub struct FrictionScale(pub f32);

/// The friction applied to [`KinematicVelocity`] when a character is grounded.
/// Multiplied by the [`FrictionScale`] of the ground entity.
#[derive(Component, Reflect, Deref, DerefMut, Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[reflect(Component, Debug, Clone)]
#[require(KinematicVelocity)]
pub struct GroundFriction(pub f32);

impl Default for GroundFriction {
    fn default() -> Self {
        Self(60.0)
    }
}

impl GroundFriction {
    pub const ZERO: Self = Self(0.0);
}

/// The drag force applied to the [`KinematicVelocity`] of a character.
#[derive(Component, Reflect, Deref, DerefMut, Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[reflect(Component, Debug, Clone)]
#[require(KinematicVelocity)]
pub struct CharacterDrag(pub f32);

impl Default for CharacterDrag {
    fn default() -> Self {
        Self(0.01)
    }
}

impl CharacterDrag {
    pub const ZERO: Self = Self(0.0);
}

#[derive(Reflect, Default, Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[reflect(Debug, Default, Clone)]
pub enum BounceBehaviour {
    Always,
    Ground,
    #[default]
    Obstruction,
    Never,
}

#[derive(Component, Reflect, Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[reflect(Component, Debug, Clone)]
pub struct CharacterBounce {
    pub restitution: f32,
    pub behaviour: BounceBehaviour,
}

impl Default for CharacterBounce {
    fn default() -> Self {
        Self {
            restitution: 0.9,
            behaviour: Default::default(),
        }
    }
}

/// The desired movement direction of a character.
/// The length of the value will be used to scale the acceleration and target speed when [`CharacterMovement`] is used.
#[derive(Component, Reflect, Default, Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[reflect(Component)]
pub struct MoveInput {
    pub value: Vec3,
    previous: Vec3,
}

impl MoveInput {
    pub fn update(&mut self) -> Vec3 {
        self.previous = mem::take(&mut self.value);
        self.previous
    }

    pub fn set(&mut self, value: Vec3) {
        self.value = value;
    }

    pub fn previous(&self) -> Vec3 {
        self.previous
    }
}

pub fn jump(impulse: f32, velocity: &mut KinematicVelocity, grounding: &mut Grounding, up: Dir3) {
    // Remove vertical velocity
    velocity.0 = velocity.0.reject_from(*up);

    // Push the character away from ramps
    if let Some(ground) = grounding.detach() {
        let impulse = ground.normal * velocity.0.dot(*ground.normal).min(0.0);

        let vertical = impulse.project_onto(*up);
        let mut horizontal = impulse - vertical;

        // Reorient horizontal impulse to original velocity direction
        if let Ok(direction) = Dir3::new(velocity.0) {
            horizontal = horizontal.project_onto(*direction);
        }

        velocity.0 -= horizontal + vertical;
    }

    velocity.0 += up * impulse;
}

pub fn feet_offset(shape: &Collider, rotation: Quat, up: Dir3, skin_width: f32) -> Vec3 {
    let aabb = shape.aabb(Vec3::ZERO, rotation);
    let down = aabb.min.dot(*up) - skin_width;
    up * down
}

pub fn feet_position(
    shape: &Collider,
    isometry: impl Into<Isometry3d>,
    up: Dir3,
    skin_width: f32,
) -> Vec3 {
    let isometry = isometry.into();
    Vec3::from(isometry.translation) + feet_offset(shape, isometry.rotation, up, skin_width)
}

#[must_use]
pub(crate) fn drag_factor(drag: f32, delta: f32) -> f32 {
    f32::exp(-drag * delta)
}

/// Constant acceleration in the opposite direction of velocity.
#[must_use]
pub(crate) fn friction_factor(velocity: Vec3, friction: f32, delta: f32) -> f32 {
    let speed_sq = velocity.length_squared();

    if speed_sq < 1e-4 {
        return 0.0;
    }

    f32::exp(-friction / speed_sq.sqrt() * delta)
}

/// Calculates acceleration towards a target speed along a given direction.
/// The resulting acceleration is clamped to avoid overshooting the target speed.
#[must_use]
pub fn acceleration(
    velocity: Vec3,
    direction: Vec3,
    max_acceleration: f32,
    target_speed: f32,
    delta: f32,
) -> Vec3 {
    // Current speed in the desired direction.
    let current_speed = velocity.dot(direction);

    // No acceleration is needed if current speed exceeds target.
    if current_speed >= target_speed {
        return Vec3::ZERO;
    }

    // Clamp to avoid acceleration past the target speed.
    let accel_speed = f32::min(target_speed - current_speed, max_acceleration * delta);

    direction * accel_speed
}

/// Factors controlling braking behavior in different directions.
#[derive(Component, Reflect, Default, Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[reflect(Component, Debug, Clone)]
pub struct BrakeFactor {
    /// Slow down backward motion when moving against the input direction.
    pub reverse: f32,
    /// Reduce lateral/sideways motion when.
    pub lateral: f32,
}

impl BrakeFactor {
    pub const ZERO: Self = Self::all(0.0);

    pub const ONE: Self = Self::all(1.0);

    pub const fn all(value: f32) -> Self {
        Self {
            reverse: value,
            lateral: value,
        }
    }
}

/// Compute the acceleration vector for the movement of a character.
///
/// For best results `velocity` should generally be constrained to the movement plane,
/// e.g., `velocity.reject_from(Vec3::Y)`.
/// Otherwise, jump height will be inconsistent when moving and standing still.
#[must_use]
pub fn acceleration_with_brake(
    velocity: Vec3,
    direction: Vec3,
    max_acceleration: f32,
    target_speed: f32,
    brake_factor: BrakeFactor,
    delta: f32,
) -> Vec3 {
    // Current speed in the desired direction.
    let current_speed = velocity.dot(direction);

    // No acceleration is needed if current speed exceeds target.
    if current_speed >= target_speed {
        return Vec3::ZERO;
    }

    // Proposed acceleration.
    let accel_speed = f32::min(target_speed - current_speed, max_acceleration * delta);
    let mut accel = accel_speed * direction;

    // No braking or clamping is needed if the velocity is almost zero.
    if velocity.length_squared() < 1e-6 {
        return accel;
    }

    // Reverse braking.
    if current_speed < 0.0 && brake_factor.reverse.abs() > 0.0 {
        let rev_brake_speed = f32::min(
            max_acceleration * brake_factor.reverse * delta,
            -current_speed,
        );
        accel += rev_brake_speed * direction;
    }

    // Lateral braking.
    if brake_factor.lateral.abs() > 0.0 {
        let perp_vel = velocity - current_speed * direction;
        accel -= perp_vel.clamp_length_max(max_acceleration * brake_factor.lateral * delta);
    }

    let new_vel = velocity + accel;

    // No clamping is needed if the new speed does not exceed the target speed.
    if new_vel.length_squared() <= target_speed * target_speed {
        return accel;
    }

    // Decompose acceleration into parallel/perpendicular components.
    let speed = velocity.length();
    let vel_dir = velocity / speed;

    let par_speed = accel.dot(vel_dir);
    let perp_accel = accel - par_speed * vel_dir;

    // Clamp parallel acceleration to stay under the target speed.
    let max_par_speed = f32::max(0.0, target_speed - speed);
    let par_accel = f32::min(max_par_speed, par_speed) * vel_dir;

    perp_accel + par_accel
}
