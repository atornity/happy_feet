use avian3d::prelude::*;
use bevy::prelude::*;

use crate::{
    grounding::find_surface_normal,
    sweep::{SweepHitData, SweepInput, collision_sweep},
};

const STEP_EPSILON: f32 = 1e-4;

#[derive(Reflect, Debug, Clone, Copy)]
#[reflect(Debug, Clone)]
pub(crate) struct StepOutput {
    pub horizontal: f32,
    pub vertical: f32,
    pub hit: SweepHitData,
}

pub(crate) fn perform_step(
    config: &SteppingConfig,
    shape: &Collider,
    origin: Vec3,
    rotation: Quat,
    direction: Dir3,
    forward_motion: f32,
    up: Dir3,
    skin_width: f32,
    query_pipeline: &SpatialQuery,
    query_filter: &SpatialQueryFilter,
    mut filter_hits: impl FnMut(&SweepHitData) -> bool,
    is_walkable: impl FnMut(&SweepHitData) -> bool,
) -> Option<StepOutput> {
    // Validate input
    if !config.is_valid() || forward_motion <= 0.0 {
        return None;
    }

    let step_up = step_up(
        shape,
        origin,
        rotation,
        up,
        skin_width,
        config.max_vertical,
        query_pipeline,
        query_filter,
        |hit| filter_hits(hit),
    )?;

    step_forward(
        config,
        shape,
        origin,
        rotation,
        direction,
        forward_motion,
        up,
        step_up,
        skin_width,
        query_pipeline,
        query_filter,
        filter_hits,
        is_walkable,
    )
}

fn step_up(
    shape: &Collider,
    origin: Vec3,
    rotation: Quat,
    up: Dir3,
    skin_width: f32,
    max_step_up: f32,
    spatial_query: &SpatialQuery,
    query_filter: &SpatialQueryFilter,
    filter_hits: impl FnMut(&SweepHitData) -> bool,
) -> Option<f32> {
    let mut step_up = max_step_up;

    if let Some(hit) = collision_sweep(
        shape,
        SweepInput {
            origin,
            rotation,
            direction: up,
            max_distance: max_step_up,
            skin_width,
            ignore_origin_penetration: false,
        },
        spatial_query,
        query_filter,
        filter_hits,
    ) {
        // Hit roof during sweep
        step_up = hit.distance.max(0.0);
    }

    // Head is already touching a roof or wall
    if step_up < STEP_EPSILON {
        return None;
    }

    Some(step_up)
}

fn step_forward(
    config: &SteppingConfig,
    shape: &Collider,
    origin: Vec3,
    rotation: Quat,
    horizontal_direction: Dir3,
    forward_motion: f32,
    up_direction: Dir3,
    step_up: f32,
    skin_width: f32,
    query_pipeline: &SpatialQuery,
    query_filter: &SpatialQueryFilter,
    mut filter_hits: impl FnMut(&SweepHitData) -> bool,
    mut validate_step: impl FnMut(&SweepHitData) -> bool,
) -> Option<StepOutput> {
    let mut min_valid_step: Option<StepOutput> = None;
    let mut min_step_forward = forward_motion;
    let mut step_forward = forward_motion;

    let step_size = config.max_horizontal / config.max_substeps.max(1) as f32;

    // Try to find the minimum step forward amount that is still steppable
    for i in 0..config.max_substeps + 1 {
        let step_up_position = origin + up_direction * step_up;

        // Sweep forward
        let mut hit_wall = false;
        if let Some(hit) = collision_sweep(
            shape,
            SweepInput {
                origin: step_up_position,
                rotation,
                direction: horizontal_direction,
                max_distance: step_forward,
                skin_width,
                ignore_origin_penetration: false,
            },
            query_pipeline,
            query_filter,
            |hit| filter_hits(hit),
        ) {
            // Already touching wall
            if hit.distance <= 0.0 {
                break;
            }

            step_forward = hit.distance;
            hit_wall = true;
        }

        let step_forward_position = step_up_position + horizontal_direction * step_forward;

        // Sweep down
        let mut valid_step = None;
        if let Some(mut hit) = collision_sweep(
            shape,
            SweepInput {
                origin: step_forward_position,
                rotation,
                direction: -up_direction,
                max_distance: step_up + skin_width,
                skin_width,
                ignore_origin_penetration: false,
            },
            query_pipeline,
            query_filter,
            |hit| filter_hits(hit),
        ) && hit.distance > 0.0
            && step_up - hit.distance > skin_width
        {
            if let Some(ray_hit) = find_surface_normal(
                hit.point,
                hit.normal,
                up_direction,
                0.01,
                query_pipeline,
                query_filter,
                |s| filter_hits(s),
            ) {
                hit.normal = ray_hit.normal;
            }

            if validate_step(&hit) {
                // We can stand here
                valid_step = Some(StepOutput {
                    horizontal: step_forward,
                    vertical: step_up - hit.distance,
                    hit,
                });

                if i == 0 {
                    return valid_step;
                }
            }
        }

        match valid_step {
            None => {
                min_step_forward = step_forward;
            }
            Some(valid_step) => {
                if let Some(last_min_valid_step) = min_valid_step.replace(valid_step)
                    && last_min_valid_step.horizontal - step_forward < STEP_EPSILON
                {
                    break;
                }
            }
        }

        if hit_wall {
            break;
        }

        match min_valid_step {
            // Step a little further
            None => step_forward += step_size,
            // Step to the middle of the furthest invalid step and the closest valid step
            Some(min_valid_step) => {
                step_forward = (min_step_forward + min_valid_step.horizontal) / 2.0;
            }
        }
    }

    min_valid_step
}

/// Determines when the character should attempt to step up.
#[derive(Reflect, Default, Debug, PartialEq, Eq, Clone, Copy)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[reflect(Default, Debug, PartialEq, Clone)]
pub enum SteppingBehaviour {
    Never,
    #[default]
    Grounded,
    Always,
}

/// Configure stepping for a character.
#[derive(Component, Reflect, Debug, PartialEq, Clone, Copy)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[reflect(Component, Default, PartialEq, Clone)]
pub struct SteppingConfig {
    pub max_vertical: f32,
    pub max_horizontal: f32,
    /// The maximum angle to be able to step on a surface, uses [`GroundingConfig`](crate::grounding::GroundingConfig) as default.
    pub max_angle: Option<f32>,
    pub max_substeps: usize,
    pub behaviour: SteppingBehaviour,
}

impl Default for SteppingConfig {
    fn default() -> Self {
        Self {
            behaviour: Default::default(),
            max_vertical: 0.25,
            max_horizontal: 0.4,
            max_angle: None,
            max_substeps: 8,
        }
    }
}

impl SteppingConfig {
    pub fn is_valid(&self) -> bool {
        self.max_vertical > 0.0 && self.max_horizontal >= 0.0
    }
}
