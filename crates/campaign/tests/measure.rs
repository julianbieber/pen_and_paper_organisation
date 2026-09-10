//! What a cell of a document is worth, and every figure a GM reads derived from it.
//!
//! Exercised without a window: what one cell is worth, how long a path is, which unit a
//! scale bar reads best in and how a figure is written are all decisions rather than
//! pixels, so all of them are checkable here.

use campaign::feature::CellPoint;
use campaign::grid::{DEFAULT_METRES_PER_CELL, TileGrid};
use campaign::manifest::{
    CAMPAIGN_VERSION, CampaignManifest, MAX_UNITS_PER_CELL, MIN_UNITS_PER_CELL,
};
use campaign::measure::{
    self, CellWorth, DistanceUnit, METRES_PER_FOOT, UnitFamily, measure_of, path_length, scale_bar,
    worth_of, worth_refusal,
};
use campaign::world::World;

fn manifest(units_per_cell: f64, unit: &str) -> CampaignManifest {
    CampaignManifest {
        version: CAMPAIGN_VERSION,
        name: "test".to_owned(),
        terrain: "terrain".to_owned(),
        units_per_cell,
        unit: unit.to_owned(),
    }
}

fn at(x: f32, y: f32) -> CellPoint {
    CellPoint::new(x, y)
}

// The world map's worth comes from campaign.ron, which before this nothing read at all.
#[test]
fn a_world_document_is_measured_by_the_campaigns_own_scale() {
    let worth = worth_of(&World::default(), &manifest(12.5, "km"));
    assert_eq!(worth.units_per_cell(), 12.5);
    assert_eq!(worth.unit(), "km");
}

// The GM asked for five feet per dungeon cell; a grid stores metres in f32, so five feet
// comes back as near to it as f32 reaches and no nearer.
#[test]
fn a_dungeon_cell_is_five_feet() {
    let grid = TileGrid::new(8, 8, DEFAULT_METRES_PER_CELL).expect("a legal grid");
    let worth = worth_of(&World::on_a_grid(grid), &manifest(12.5, "km"));
    assert_eq!(worth.unit(), "ft");
    assert!((worth.units_per_cell() - 5.0).abs() < 1e-6, "{}", worth.units_per_cell());
}

// A dungeon ignores the campaign's unit entirely, which is what makes the figures readable.
#[test]
fn a_dungeon_ignores_the_campaigns_unit() {
    let grid = TileGrid::new(8, 8, DEFAULT_METRES_PER_CELL).expect("a legal grid");
    let dungeon = worth_of(&World::on_a_grid(grid), &manifest(3.0, "leagues"));
    assert_eq!(dungeon.unit(), "ft");
}

// No travel time in a dungeon: a pace in days says nothing about a corridor.
#[test]
fn a_dungeon_carries_no_travel_time() {
    let grid = TileGrid::new(8, 8, DEFAULT_METRES_PER_CELL).expect("a legal grid");
    let worth = worth_of(&World::on_a_grid(grid), &manifest(1.0, "km"));
    assert!(!worth.travelled());

    let measured = measure_of(&[at(0.0, 0.0), at(10.0, 0.0)], false, &worth, 30.0)
        .expect("two points are a measurement");
    assert_eq!(measured.path.days, None);
    assert_eq!(measured.straight.expect("an open path").days, None);
}

// A polygon does not store its closing edge, so a sum over its vertices is one edge short.
#[test]
fn a_polygons_length_includes_the_edge_it_does_not_store() {
    let square = [at(0.0, 0.0), at(1.0, 0.0), at(1.0, 1.0), at(0.0, 1.0)];
    assert_eq!(path_length(&square, false), 3.0);
    assert_eq!(path_length(&square, true), 4.0);
}

// A closed shape has no "straight line between the ends" — first-to-last is just one edge.
#[test]
fn a_closed_path_is_never_shown_beside_a_straight_line() {
    let worth = worth_of(&World::default(), &manifest(1.0, "km"));
    let square = [at(0.0, 0.0), at(1.0, 0.0), at(1.0, 1.0), at(0.0, 1.0)];
    let measured = measure_of(&square, true, &worth, 30.0).expect("a measurement");
    assert_eq!(measured.straight, None);
    assert_eq!(measured.path.distance, 4.0);
}

// The acceptance criterion for a road: its own length exceeds the line across it.
#[test]
fn a_bent_road_is_longer_than_the_line_across_it() {
    let worth = worth_of(&World::default(), &manifest(2.0, "km"));
    let road = [at(0.0, 0.0), at(3.0, 4.0), at(6.0, 0.0)];
    let measured = measure_of(&road, false, &worth, 30.0).expect("a measurement");
    let straight = measured.straight.expect("an open path");
    assert_eq!(measured.path.distance, 20.0);
    assert_eq!(straight.distance, 12.0);
    assert!(measured.path.distance > straight.distance);
}

// A straight line agrees with units_per_cell times the cell distance, which is acceptance 2.
#[test]
fn a_straight_line_is_the_scale_times_the_cell_distance() {
    let worth = worth_of(&World::default(), &manifest(2.5, "km"));
    let measured = measure_of(&[at(0.0, 0.0), at(3.0, 4.0)], false, &worth, 30.0)
        .expect("two points are a measurement");
    assert_eq!(measured.path.distance, 5.0 * 2.5);
}

// Fewer than two points is not a measurement, which is what keeps a first click silent.
#[test]
fn one_point_is_not_a_measurement() {
    let worth = worth_of(&World::default(), &manifest(1.0, "km"));
    assert!(measure_of(&[at(0.0, 0.0)], false, &worth, 30.0).is_none());
    assert!(measure_of(&[], false, &worth, 30.0).is_none());
}

// Travel time is the distance over the pace, and the slider's floor keeps the pace real.
#[test]
fn travel_time_is_the_distance_over_the_pace() {
    let worth = worth_of(&World::default(), &manifest(10.0, "km"));
    let measured = measure_of(&[at(0.0, 0.0), at(6.0, 0.0)], false, &worth, 30.0)
        .expect("a measurement");
    assert_eq!(measured.path.days, Some(2.0));
}

// A pace of zero cannot arrive from the slider, but the model must not answer an infinity.
#[test]
fn a_pace_of_nothing_is_no_travel_time_rather_than_an_infinity() {
    let worth = worth_of(&World::default(), &manifest(1.0, "km"));
    for pace in [0.0, -5.0, f64::NAN, f64::INFINITY] {
        let measured =
            measure_of(&[at(0.0, 0.0), at(1.0, 0.0)], false, &worth, pace).expect("a measurement");
        assert_eq!(measured.path.days, None, "pace {pace}");
    }
}

// The bar's label is 1, 2 or 5 times a power of ten, at every zoom the camera admits.
#[test]
fn every_bar_is_labelled_a_round_figure() {
    let worth = worth_of(&World::default(), &manifest(1.0, "km"));
    let mut units_per_pixel = 1.0e-6;
    while units_per_pixel < 1.0e6 {
        let (figure, _, _) = scale_bar(units_per_pixel, &worth, 480.0).expect("a bar");
        let normal = figure / 10f64.powf(figure.log10().floor());
        assert!(
            (normal - 1.0).abs() < 1e-9 || (normal - 2.0).abs() < 1e-9 || (normal - 5.0).abs() < 1e-9,
            "{figure} at {units_per_pixel} per pixel normalises to {normal}"
        );
        units_per_pixel *= 1.7;
    }
}

// "Sensible" in checkable form: consecutive round figures are at most 2.5x apart, so the
// bar can never shrink to a sliver or overflow the quarter it is allowed.
#[test]
fn the_bar_is_between_a_tenth_and_a_quarter_of_what_it_may_use() {
    let worth = worth_of(&World::default(), &manifest(1.0, "km"));
    let mut units_per_pixel = 1.0e-6;
    while units_per_pixel < 1.0e6 {
        let (_, _, pixels) = scale_bar(units_per_pixel, &worth, 480.0).expect("a bar");
        assert!(
            pixels > 480.0 / 2.5 - 0.001 && pixels <= 480.001,
            "{pixels} pixels at {units_per_pixel} per pixel"
        );
        units_per_pixel *= 1.7;
    }
}

// The GM asked for the bar to change unit as the map is zoomed in.
#[test]
fn a_kilometre_campaign_reads_in_metres_when_zoomed_into_a_city() {
    let worth = worth_of(&World::default(), &manifest(1.0, "km"));
    let (_, far, _) = scale_bar(1.0, &worth, 480.0).expect("a bar");
    let (_, close, _) = scale_bar(1.0e-4, &worth, 480.0).expect("a bar");
    assert_eq!(far, "km");
    assert_eq!(close, "m");
}

// A step never crosses families, so a campaign in miles never reads in metres.
#[test]
fn a_step_stays_inside_its_own_family() {
    let worth = worth_of(&World::default(), &manifest(1.0, "miles"));
    let mut units_per_pixel = 1.0e-6;
    while units_per_pixel < 1.0e4 {
        let (_, label, _) = scale_bar(units_per_pixel, &worth, 480.0).expect("a bar");
        let unit = DistanceUnit::from_label(&label).expect("a known unit");
        assert_eq!(unit.family(), UnitFamily::Imperial, "{label}");
        units_per_pixel *= 1.7;
    }
}

// A campaign may carry any label at all; one this build does not know simply cannot step.
#[test]
fn an_unknown_unit_keeps_its_label_at_every_zoom() {
    let worth = worth_of(&World::default(), &manifest(1.0, "spans"));
    assert_eq!(worth.known_unit(), None);
    for units_per_pixel in [1.0e-4, 1.0, 1.0e4] {
        let (_, label, _) = scale_bar(units_per_pixel, &worth, 480.0).expect("a bar");
        assert_eq!(label, "spans");
    }
}

// A map with no scale to draw is an answer, not a fault — the bar is taken off screen.
#[test]
fn a_zoom_that_is_not_a_number_draws_no_bar() {
    let worth = worth_of(&World::default(), &manifest(1.0, "km"));
    assert!(scale_bar(0.0, &worth, 480.0).is_none());
    assert!(scale_bar(f64::NAN, &worth, 480.0).is_none());
    assert!(scale_bar(1.0, &worth, 0.0).is_none());
}

// The bounds exist so the round-figure search cannot run away; check they are enforced.
#[test]
fn a_scale_beyond_the_bounds_is_refused() {
    assert!(worth_refusal(1.0).is_none());
    assert!(worth_refusal(MIN_UNITS_PER_CELL).is_none());
    assert!(worth_refusal(MAX_UNITS_PER_CELL).is_none());
    assert!(worth_refusal(0.0).is_some());
    assert!(worth_refusal(-1.0).is_some());
    assert!(worth_refusal(f64::NAN).is_some());
    assert!(worth_refusal(f64::INFINITY).is_some());
    assert!(worth_refusal(MIN_UNITS_PER_CELL / 10.0).is_some());
    assert!(worth_refusal(MAX_UNITS_PER_CELL * 10.0).is_some());
}

// A foot is 0.3048 metres by definition, and the conversion must not drift from that.
#[test]
fn feet_convert_by_the_exact_definition() {
    assert_eq!(DistanceUnit::Feet.metres(), METRES_PER_FOOT);
    assert_eq!(DistanceUnit::Kilometres.metres(), 1000.0);
    let grid = TileGrid::new(4, 4, 3.048).expect("a legal grid");
    let worth = worth_of(&World::on_a_grid(grid), &manifest(1.0, "km"));
    assert!((worth.units_per_cell() - 10.0).abs() < 1e-6, "{}", worth.units_per_cell());
}

// The label a unit writes is the label it is read back from, or the dropdown cannot work.
#[test]
fn every_unit_round_trips_through_its_label() {
    for unit in DistanceUnit::all() {
        assert_eq!(DistanceUnit::from_label(unit.label()), Some(unit));
    }
    assert_eq!(DistanceUnit::from_label("KM"), Some(DistanceUnit::Kilometres));
    assert_eq!(DistanceUnit::from_label("furlongs"), None);
}

// One formatter, so the bar, the ruler and the panel cannot round differently.
#[test]
fn a_figure_is_written_with_its_unit() {
    assert_eq!(measure::figure(1234.0, "km"), "1234 km");
    assert_eq!(measure::figure(12.34, "km"), "12.3 km");
    assert_eq!(measure::figure(1.234, "km"), "1.23 km");
}

// A CellWorth is the one place a cell count becomes a figure, so pin the widening.
#[test]
fn a_cell_count_becomes_a_figure_through_the_worth_alone() {
    let worth: CellWorth = worth_of(&World::default(), &manifest(2.0, "km"));
    assert_eq!(worth.units(3.5), 7.0);
}
