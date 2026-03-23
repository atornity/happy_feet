use avian3d::prelude::*;
use bevy::prelude::*;

#[derive(Reflect, Debug, Clone, Copy)]
#[reflect(Debug, Clone)]
pub struct SweepInput {
    pub origin: Vec3,
    pub rotation: Quat,
    pub direction: Dir3,
    pub max_distance: f32,
    pub skin_width: f32,
    pub ignore_origin_penetration: bool,
}

#[derive(Reflect, Debug, Clone, Copy)]
#[reflect(Debug, Clone)]
pub struct SweepHitData {
    pub distance: f32,
    pub point: Vec3,
    pub normal: Vec3,
    pub entity: Entity,
}

/// Returns the safe hit distance and the hit data from the spatial query for the first hit where `f` returns `true`.
#[must_use]
pub fn collision_sweep(
    shape: &Collider,
    SweepInput {
        origin,
        rotation,
        direction,
        max_distance,
        skin_width,
        ignore_origin_penetration,
    }: SweepInput,
    query_pipeline: &SpatialQuery,
    query_filter: &SpatialQueryFilter,
    mut filter_hits: impl FnMut(&SweepHitData) -> bool,
) -> Option<SweepHitData> {
    let mut hit = None;
    query_pipeline.shape_hits_callback(
        shape,
        origin,
        rotation,
        direction,
        &ShapeCastConfig {
            max_distance: max_distance + skin_width, // extend the trace slightly
            target_distance: skin_width, // I'm not sure what this does, but I think this is correct ;)
            ignore_origin_penetration,
            ..Default::default()
        },
        query_filter,
        |h| {
            // How far is safe to translate by
            let distance = h.distance - skin_width;

            let h = SweepHitData {
                distance,
                point: h.point1,
                normal: h.normal1,
                entity: h.entity,
            };

            if filter_hits(&h) {
                hit = Some(h);
            }

            // Continue the sweep until we find a hit that passes the filter
            hit.is_none()
        },
    );

    hit
}
