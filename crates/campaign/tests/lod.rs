//! How much detail a feature is drawn at: that the reveal is monotone in zoom, that a pan
//! cannot change it, and that no shape of parent chain can make a feature disappear when
//! it should not.

use campaign::edit::Edit;
use campaign::feature::{CellPoint, Feature, FeatureId, FeatureKind, Geometry};
use campaign::lod::{self, Areas, DETAIL_AREA_SQUARE_PIXELS, DETAIL_FADE_SPAN, MIN_PICKABLE_DETAIL};
use campaign::world::World;

fn at(x: f32, y: f32) -> CellPoint {
    CellPoint::new(x, y)
}

fn add(world: &mut World, feature: Feature) -> FeatureId {
    let id = world.fresh_id();
    Edit::Add { id, feature }
        .apply(world)
        .expect("the fixture must be addable, or it tests nothing");
    id
}

fn city_with_a_tavern(side: f32) -> (World, FeatureId, FeatureId) {
    let mut world = World::default();
    let city = add(
        &mut world,
        Feature::plain(
            FeatureKind::Settlement,
            Geometry::Polygon(vec![
                at(0.0, 0.0),
                at(side, 0.0),
                at(side, side),
                at(0.0, side),
            ]),
        ),
    );
    let tavern = add(
        &mut world,
        Feature {
            parent: Some(city),
            ..Feature::plain(FeatureKind::Poi, Geometry::Point(at(side / 2.0, side / 2.0)))
        },
    );
    (world, city, tavern)
}

fn scale_for(side: f32, square_pixels: f32) -> f32 {
    (side * side / square_pixels).sqrt()
}

// The whole of the city-reveal criterion: zoomed out the tavern is not drawn at all,
// zoomed in it is drawn in full, and the city itself is never gated by a parent it does
// not have.
#[test]
fn a_tavern_is_hidden_until_its_city_covers_enough_of_the_screen() {
    let (world, city, tavern) = city_with_a_tavern(40.0);
    let areas = Areas::of(&world);

    let far = scale_for(40.0, DETAIL_AREA_SQUARE_PIXELS / 4.0);
    let near = scale_for(40.0, DETAIL_AREA_SQUARE_PIXELS * DETAIL_FADE_SPAN * 2.0);

    assert_eq!(lod::detail(&world, &areas, far, tavern, &[]), 0.0);
    assert_eq!(lod::detail(&world, &areas, near, tavern, &[]), 1.0);

    assert_eq!(
        lod::detail(&world, &areas, far, city, &[]),
        1.0,
        "a feature with no parent is drawn in full at every zoom it has not opted out of"
    );
}

// Nothing may appear or vanish as the camera moves, so the reveal has to be a ramp rather
// than a step — and it has to run the right way, or zooming in would hide things.
#[test]
fn revealing_is_monotone_in_zoom_and_never_steps() {
    let (world, _, tavern) = city_with_a_tavern(40.0);
    let areas = Areas::of(&world);

    let mut previous = 0.0;
    let mut partial = 0;
    for step in 0..=60 {
        let scale = scale_for(40.0, DETAIL_AREA_SQUARE_PIXELS * (0.5 + step as f32 * 0.1));
        let detail = lod::detail(&world, &areas, scale, tavern, &[]);
        assert!(
            detail >= previous - 1e-5,
            "detail fell from {previous} to {detail} while zooming in"
        );
        if detail > 0.0 && detail < 1.0 {
            partial += 1;
        }
        previous = detail;
    }
    assert!(
        partial > 4,
        "the reveal stepped rather than fading: only {partial} partial values across the span"
    );
}

// A polygon's area is its whole shoelace area rather than the part on screen, which is
// what makes the answer depend on the zoom alone. If it were clipped to the view, panning
// a city half off screen would start hiding the buildings inside it. Translating the whole
// document is what a pan is, seen from the document's side.
#[test]
fn panning_cannot_change_how_much_detail_a_feature_is_drawn_at() {
    let (mut world, _, tavern) = city_with_a_tavern(40.0);
    let areas = Areas::of(&world);
    let scale = scale_for(40.0, DETAIL_AREA_SQUARE_PIXELS * 2.0);
    let before = lod::detail(&world, &areas, scale, tavern, &[]);

    let ids: Vec<FeatureId> = world.features().map(|(id, _)| id).collect();
    let edit = campaign::gesture::translate(&world, &ids, at(10_000.0, -7_000.0));
    edit.apply(&mut world).expect("a translate must apply");
    let moved = Areas::of(&world);

    assert_eq!(lod::detail(&world, &moved, scale, tavern, &[]), before);
}

// A degenerate parent must never swallow its children: a POI parented to a point, or to
// the three collinear vertices below — a legal polygon enclosing no area at all — would
// otherwise be invisible at every zoom, with nothing on screen to say why.
#[test]
fn a_parent_that_encloses_nothing_constrains_nothing() {
    let mut world = World::default();
    let marker = add(
        &mut world,
        Feature::plain(FeatureKind::Settlement, Geometry::Point(at(0.0, 0.0))),
    );
    let flat = add(
        &mut world,
        Feature::plain(
            FeatureKind::Territory,
            Geometry::Polygon(vec![at(0.0, 0.0), at(10.0, 0.0), at(20.0, 0.0)]),
        ),
    );
    let under_point = add(
        &mut world,
        Feature {
            parent: Some(marker),
            ..Feature::plain(FeatureKind::Poi, Geometry::Point(at(0.0, 0.0)))
        },
    );
    let under_flat = add(
        &mut world,
        Feature {
            parent: Some(flat),
            ..Feature::plain(FeatureKind::Poi, Geometry::Point(at(5.0, 0.0)))
        },
    );
    let areas = Areas::of(&world);

    for id in [under_point, under_flat] {
        assert_eq!(
            lod::detail(&world, &areas, 1000.0, id, &[]),
            1.0,
            "a degenerate ancestor hid its child"
        );
    }
}

// Detail is the least over the whole chain, not just the nearest parent — a cellar inside
// a tavern inside a city must not appear before the city it is in does.
#[test]
fn the_whole_parent_chain_constrains_a_feature_not_just_its_parent() {
    let (mut world, _, tavern) = city_with_a_tavern(40.0);
    let cellar = add(
        &mut world,
        Feature {
            parent: Some(tavern),
            ..Feature::plain(FeatureKind::DungeonEntry, Geometry::Point(at(20.0, 20.0)))
        },
    );
    let areas = Areas::of(&world);
    let far = scale_for(40.0, DETAIL_AREA_SQUARE_PIXELS / 4.0);

    assert_eq!(lod::detail(&world, &areas, far, cellar, &[]), 0.0);
}

// A feature's own threshold is in cells per logical pixel and hides it when the map is
// coarser than that, whatever its parents allow.
#[test]
fn a_feature_hides_itself_once_the_map_is_coarser_than_its_own_threshold() {
    let mut world = World::default();
    let trail = add(
        &mut world,
        Feature {
            max_cells_per_pixel: Some(0.5),
            ..Feature::plain(
                FeatureKind::Trail,
                Geometry::Polyline(vec![at(0.0, 0.0), at(10.0, 10.0)]),
            )
        },
    );
    let areas = Areas::of(&world);

    assert_eq!(lod::detail(&world, &areas, 0.25, trail, &[]), 1.0, "finer than asked");
    assert_eq!(lod::detail(&world, &areas, 0.5, trail, &[]), 1.0, "exactly as asked");
    assert_eq!(lod::detail(&world, &areas, 100.0, trail, &[]), 0.0, "far coarser");

    let mid = lod::detail(&world, &areas, 0.65, trail, &[]);
    assert!(mid > 0.0 && mid < 1.0, "the threshold stepped instead of fading: {mid}");
}

// A selected feature has to stay drawable and clickable at any zoom, or there is no way to
// click it again to deselect it. The exemption lives inside `detail` so that drawing and
// picking cannot disagree about it.
#[test]
fn a_selected_feature_is_exempt_from_both_thresholds() {
    let (world, _, tavern) = city_with_a_tavern(40.0);
    let areas = Areas::of(&world);
    let far = scale_for(40.0, DETAIL_AREA_SQUARE_PIXELS / 100.0);

    assert_eq!(lod::detail(&world, &areas, far, tavern, &[]), 0.0);
    assert_eq!(lod::detail(&world, &areas, far, tavern, &[tavern]), 1.0);
    assert!(lod::is_pickable(&world, &areas, far, tavern, &[tavern]));
}

// What can be picked is what is drawn, and the floor is a perceptibility floor rather than
// "anything above zero": the tavern below is drawn, just inside the fade and only faintly,
// and a click on something that faint must still select nothing.
#[test]
fn a_barely_visible_feature_cannot_be_clicked() {
    let (world, _, tavern) = city_with_a_tavern(40.0);
    let areas = Areas::of(&world);

    let faint = scale_for(
        40.0,
        DETAIL_AREA_SQUARE_PIXELS * (1.0 + (DETAIL_FADE_SPAN - 1.0) * 0.02),
    );
    let detail = lod::detail(&world, &areas, faint, tavern, &[]);
    assert!(detail > 0.0 && detail < MIN_PICKABLE_DETAIL, "fixture is not faint: {detail}");
    assert!(!lod::is_pickable(&world, &areas, faint, tavern, &[]));
}

// The cache is rebuilt rather than updated, because an edit can delete a feature as easily
// as move a vertex — a table that only gained entries would answer for features the
// document no longer holds and keep them hidden or revealed on stale geometry.
#[test]
fn rebuilding_the_area_cache_forgets_what_the_document_dropped() {
    let (mut world, city, tavern) = city_with_a_tavern(40.0);
    let mut areas = Areas::of(&world);
    assert_eq!(areas.len(), 1, "only the polygon is measured");
    assert!((areas.of_feature(city) - 1600.0).abs() < 0.01);

    Edit::Delete { id: tavern }.apply(&mut world).expect("deletable");
    Edit::Delete { id: city }.apply(&mut world).expect("deletable");
    areas.rebuild(&world);

    assert!(areas.is_empty());
    assert_eq!(areas.of_feature(city), 0.0, "an absent area constrains nothing");
}

// A camera that has not been framed yet reaches here with a scale of zero, and a NaN
// threshold would compare false against everything — neither may hang the frame or answer
// something the caller then multiplies an alpha by.
#[test]
fn an_unusable_scale_answers_zero_rather_than_a_nan() {
    let (world, _, tavern) = city_with_a_tavern(40.0);
    let areas = Areas::of(&world);

    for scale in [0.0, -1.0, f32::NAN] {
        let detail = lod::detail(&world, &areas, scale, tavern, &[]);
        assert_eq!(detail, 0.0, "a scale of {scale} answered {detail}");
    }
    assert_eq!(
        lod::detail(&world, &areas, 1.0, FeatureId(9999), &[]),
        0.0,
        "an id the world does not hold has nothing to draw"
    );
}
