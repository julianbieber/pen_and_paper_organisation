//! The one runtime control over what counts as a river.

use bevy::feathers::controls::FeathersSlider;
use bevy::feathers::theme::{ThemeBackgroundColor, ThemedText};
use bevy::feathers::tokens;
use bevy::prelude::*;
use bevy::scene::CommandsSceneExt;
use bevy::ui_widgets::{SliderValue, slider_self_update};

use crate::map::load::MapTerrain;

/// Where the threshold starts, as a fraction of the accumulation the terrain reaches.
///
/// Not zero: `watershed` reports zero accumulation off the edge of the terrain and
/// tests channels with `>=`, so a threshold there would draw the whole map — and
/// everything beyond it — as river.
pub const DEFAULT_THRESHOLD_FRACTION: f32 = 0.05;

/// The accumulation above which a land cell is drawn as a channel.
///
/// The single control deciding whether the map reads as a drainage basin or a puddle,
/// which is why it is a value at runtime and not a constant. Starts above every
/// accumulation there is, so nothing is drawn as a river until a terrain has been read
/// and the threshold has a scale to mean something against.
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

/// The slider the threshold is read from.
#[derive(Component, Default, Clone)]
pub struct ThresholdSlider;

/// Hangs the river-threshold panel over the map, once there is a map to control.
///
/// The slider spans the accumulation this terrain actually reaches: accumulation
/// counts everything draining through a cell, so it scales with the terrain, and a
/// fixed range would be all-river on one terrain and all-dry on the next.
pub fn build_map_panel(
    mut commands: Commands,
    terrain: Res<MapTerrain>,
    mut threshold: ResMut<RiverThreshold>,
) {
    let ceiling = terrain.accumulation_high.max(f32::MIN_POSITIVE);
    threshold.accumulation = default_threshold(ceiling);
    commands.spawn_scene(panel(threshold.accumulation, ceiling));
}

fn default_threshold(ceiling: f32) -> f32 {
    (ceiling * DEFAULT_THRESHOLD_FRACTION).max(f32::MIN_POSITIVE)
}

fn panel(value: f32, ceiling: f32) -> impl Scene {
    bsn! {
        Node {
            position_type: PositionType::Absolute,
            bottom: px(40),
            left: px(12),
            display: Display::Flex,
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            column_gap: px(8),
            padding: px(8),
            width: px(420),
        }
        ThemeBackgroundColor(tokens::WINDOW_BG)
        Children [
            (Text("Rivers above") ThemedText),
            (
                @FeathersSlider {
                    @value: {value},
                    @min: 0.0,
                    @max: {ceiling},
                }
                ThresholdSlider
                on(slider_self_update)
            )
        ]
    }
}

/// Lands a moved slider on [`RiverThreshold`].
///
/// Writes only when the value actually differs: an unconditional write marks the
/// resource changed every frame, and the refill watching it would then rebuild every
/// resident chunk for the life of the process.
pub fn land_threshold(
    sliders: Query<&SliderValue, With<ThresholdSlider>>,
    mut threshold: ResMut<RiverThreshold>,
) {
    for value in sliders.iter() {
        if threshold.accumulation != value.0 {
            threshold.accumulation = value.0;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Resource, Default)]
    struct Probe {
        moved: bool,
    }

    fn probe(threshold: Res<RiverThreshold>, mut seen: ResMut<Probe>) {
        seen.moved = threshold.is_changed();
    }

    fn app_with(value: f32) -> (App, Entity) {
        let mut app = App::new();
        app.init_resource::<RiverThreshold>()
            .init_resource::<Probe>()
            .add_systems(Update, (land_threshold, probe).chain());
        let slider = app.world_mut().spawn((ThresholdSlider, SliderValue(value))).id();
        (app, slider)
    }

    // The single most expensive mistake available here: an unconditional `ResMut`
    // write marks the resource changed every frame, and the refill watching it would
    // then rebuild every resident chunk for the life of the process.
    #[test]
    fn a_slider_that_has_not_moved_does_not_mark_the_threshold_changed() {
        let (mut app, _) = app_with(120.0);

        app.update();
        app.update();

        assert_eq!(app.world().resource::<RiverThreshold>().accumulation, 120.0);
        assert!(
            !app.world().resource::<Probe>().moved,
            "a still slider must leave the threshold untouched"
        );
    }

    // And it must still land a real move, or the control does nothing at all.
    #[test]
    fn a_moved_slider_lands_on_the_threshold() {
        let (mut app, slider) = app_with(120.0);
        app.update();
        app.update();

        app.world_mut().entity_mut(slider).insert(SliderValue(900.0));
        app.update();

        assert_eq!(app.world().resource::<RiverThreshold>().accumulation, 900.0);
        assert!(app.world().resource::<Probe>().moved);
    }

    // Zero is the one value that must not be the starting threshold: `watershed`
    // answers zero accumulation off the edge of the terrain, so a threshold there
    // draws the whole map — and everything beyond it — as river.
    #[test]
    fn the_threshold_never_starts_at_zero_whatever_the_terrain() {
        for ceiling in [0.0, f32::MIN_POSITIVE, 1.0, 10_000.0, f32::MAX] {
            let start = default_threshold(ceiling);
            assert!(start > 0.0, "ceiling {ceiling} started the threshold at {start}");
            assert!(start.is_finite() || ceiling == f32::MAX);
        }
        assert!(default_threshold(10_000.0) < 10_000.0, "and below the ceiling");
    }
}
