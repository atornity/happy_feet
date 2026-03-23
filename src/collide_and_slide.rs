use avian3d::prelude::*;
use bevy::{ecs::relationship::RelationshipSourceCollection, prelude::*};

use crate::{
    grounding::Ground,
    projection::Surface,
    sweep::{SweepHitData, SweepInput, collision_sweep},
};

pub fn collide_and_slide(
    state: &mut MovementState,
    shape: &Collider,
    rotation: Quat,
    config: &CollideAndSlideConfig,
    query_pipeline: &SpatialQuery,
    filter: &SpatialQueryFilter,
    mut get_surface: impl FnMut(&SweepHitData) -> Option<Surface>,
    mut on_hit: impl FnMut(&mut MovementState, SlideInfo) -> CollisionResponse,
    mut project_velocity: impl FnMut(Vec3, Surface) -> Vec3,
) {
    assert!(config.max_penetration_retraction >= 0.0);

    for _ in 0..config.max_iterations {
        let Ok((direction, max_distance)) =
            Dir3::new_and_length(state.velocity * state.remaining_time)
        else {
            break;
        };

        let origin = state.position();

        let input = SweepInput {
            origin,
            rotation,
            direction,
            max_distance,
            skin_width: config.skin_width,
            ignore_origin_penetration: true,
        };

        let mut surface = None;
        let Some(hit) = collision_sweep(shape, input, query_pipeline, filter, |hit| {
            if let Some(s) = get_surface(hit) {
                surface = Some(s);
                true
            } else {
                false
            }
        }) else {
            state.offset += direction * max_distance;
            break;
        };

        let surface = surface.unwrap();
        let distance = hit.distance.max(-config.max_penetration_retraction);

        state.remaining_time *= 1.0 - distance / max_distance;
        state.offset += direction * distance;

        if surface.is_walkable {
            state.ground = Some(Ground::new(hit.entity, surface.normal));
        }

        match on_hit(
            state,
            SlideInfo {
                input,
                hit,
                surface,
            },
        ) {
            CollisionResponse::Slide => {}
            CollisionResponse::Skip => continue,
            CollisionResponse::Stop => break,
        }

        state.velocity = project_velocity(state.velocity, surface);
    }
}

#[derive(Reflect, Debug, Clone, Copy)]
#[reflect(Debug, Clone)]
pub struct SlideInfo {
    pub input: SweepInput,
    pub hit: SweepHitData,
    pub surface: Surface,
}

#[derive(Reflect, Debug, PartialEq, Clone, Copy)]
#[reflect(Debug, PartialEq, Clone)]
pub enum CollisionResponse {
    Slide,
    Skip,
    Stop,
}

#[derive(Reflect, Debug, Clone, Copy)]
#[reflect(Debug, Clone)]
pub struct MovementState {
    pub velocity: Vec3,
    pub origin: Vec3,
    pub offset: Vec3,
    pub remaining_time: f32,
    pub ground: Option<Ground>,
}

impl MovementState {
    pub fn new(velocity: Vec3, position: Vec3, duration: f32) -> Self {
        Self {
            velocity,
            origin: position,
            offset: Vec3::ZERO,
            remaining_time: duration,
            ground: None,
        }
    }

    pub fn position(&self) -> Vec3 {
        self.origin + self.offset
    }
}

/// Configuration parameters for [`collide_and_slide`] movement.
///
/// Can be configured globally via the resource or per-entity using a component.
#[derive(Resource, Component, Reflect, Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[reflect(Component, Default)]
#[require(CollideAndSlideFilter)]
pub struct CollideAndSlideConfig {
    /// Maximum number of collision resolution attempts per update. Default: `4`
    pub max_iterations: u8,
    /// Small outward offset from the collision surface to prevent numerical precision issues. Default: `0.05`
    pub skin_width: f32,
    /// Maximum distance to push back when resolving penetration. Must not be negative. Default: `0.0`
    pub max_penetration_retraction: f32,
}

impl Default for CollideAndSlideConfig {
    fn default() -> Self {
        Self {
            max_iterations: 4,
            skin_width: 0.05,
            max_penetration_retraction: 0.0,
        }
    }
}

/// Cache the [`SpatialQueryFilter`] of the character to avoid re-allocating the excluded entities map every time it's used.
#[derive(Component, Reflect, Deref, Default, Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[reflect(Component)]
pub struct CollideAndSlideFilter(pub(crate) SpatialQueryFilter);

pub(crate) fn init_filter_mask(
    trigger: On<Insert, CollisionLayers>,
    mut rigid_bodies: Query<(&mut CollideAndSlideFilter, &CollisionLayers)>,
) {
    let Ok((mut filter, layers)) = rigid_bodies.get_mut(trigger.event().entity) else {
        return;
    };

    filter.0.mask = layers.filters;
}

pub(crate) fn add_collider_to_filter(
    trigger: On<Insert, ColliderOf>,
    colliders: Query<&ColliderOf>,
    mut filters: Query<&mut CollideAndSlideFilter>,
) {
    let collider_of = colliders.get(trigger.event().entity).unwrap();

    let Ok(mut filters) = filters.get_mut(collider_of.body) else {
        return;
    };

    filters.0.excluded_entities.insert(trigger.event().entity);
}

pub(crate) fn remove_collider_from_filter<T: EntityEvent>(
    trigger: On<T, ColliderOf>,
    colliders: Query<&ColliderOf>,
    mut filters: Query<&mut CollideAndSlideFilter>,
) {
    let collider_of = colliders.get(trigger.event().event_target()).unwrap();

    let Ok(mut filters) = filters.get_mut(collider_of.body) else {
        return;
    };

    filters
        .0
        .excluded_entities
        .remove(trigger.event().event_target());
}
