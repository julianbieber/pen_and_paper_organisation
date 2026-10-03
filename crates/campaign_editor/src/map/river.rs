//! What counts as a river on the terrain.

use bevy::prelude::*;

use crate::map::load::MapTerrain;

/// Where the threshold sits, as a fraction of the accumulation the terrain reaches.
///
/// Not zero: `watershed` reports zero accumulation off the edge of the terrain and
/// tests channels with `>=`, so a threshold there would draw the whole map — and
/// everything beyond it — as river.
pub const THRESHOLD_FRACTION: f32 = 0.05;

/// The accumulation above which a land cell is drawn as a channel.
///
/// Starts above every accumulation there is, so nothing is drawn as a river until a
/// terrain has been read and the threshold has a scale to mean something against.
#[derive(Resource, Debug, Clone, Copy)]
pub struct RiverThreshold {
    pub accumulation: f32,
}

impl Default for RiverThreshold {
    fn default() -> Self {
        Self {
            accumulation: f32::MAX,
        }
    }
}

/// Sets the threshold for a terrain that has just been read.
///
/// Accumulation counts everything draining through a cell, so it scales with the terrain
/// and the threshold is a fraction of what this one reaches.
pub fn set_threshold(terrain: Res<MapTerrain>, mut threshold: ResMut<RiverThreshold>) {
    threshold.accumulation = threshold_for(terrain.accumulation_high);
}

fn threshold_for(ceiling: f32) -> f32 {
    (ceiling.max(f32::MIN_POSITIVE) * THRESHOLD_FRACTION).max(f32::MIN_POSITIVE)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Zero is the one value the threshold must never take: `watershed` answers zero
    // accumulation off the edge of the terrain, so a threshold there draws the whole map
    // — and everything beyond it — as river.
    #[test]
    fn the_threshold_is_never_zero_whatever_the_terrain() {
        for ceiling in [0.0, f32::MIN_POSITIVE, 1.0, 10_000.0, f32::MAX] {
            let threshold = threshold_for(ceiling);
            assert!(threshold > 0.0, "ceiling {ceiling} gave {threshold}");
        }
        assert!(threshold_for(10_000.0) < 10_000.0, "and below the ceiling");
    }
}
