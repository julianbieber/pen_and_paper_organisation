//! Where a label sits and which labels survive a collision — the acceptance criterion
//! that no two labels overlap at any zoom, checked without drawing a glyph.

use campaign::feature::{CellPoint, FeatureId, Geometry};
use campaign::label::{self, LabelBox, LabelCandidate, MAX_LABELS};

fn at(x: f32, y: f32) -> CellPoint {
    CellPoint::new(x, y)
}

fn candidate(id: u64, x: f32, y: f32, priority: u32) -> LabelCandidate {
    LabelCandidate {
        feature: FeatureId(id),
        anchor: at(x, y),
        priority,
        fade: 1.0,
    }
}

fn box_of(width: f32, height: f32) -> LabelBox {
    LabelBox {
        width_pixels: width,
        height_pixels: height,
    }
}

// The acceptance criterion, stated directly: offered a grid of candidates far denser than
// their boxes allow, no two labels that are actually drawn may overlap.
#[test]
fn no_two_placed_labels_overlap() {
    let mut candidates = Vec::new();
    let mut boxes = Vec::new();
    for row in 0..20 {
        for column in 0..20 {
            let id = row * 20 + column;
            candidates.push(candidate(id, column as f32 * 4.0, row as f32 * 4.0, id as u32));
            boxes.push(box_of(30.0, 14.0));
        }
    }

    let placed = label::place(&candidates, &boxes, 1.0, MAX_LABELS);
    assert!(!placed.is_empty(), "a dense field must still place something");

    for (index, one) in placed.iter().enumerate() {
        for other in placed.iter().skip(index + 1) {
            assert!(
                !box_of(30.0, 14.0).meets(one.anchor, box_of(30.0, 14.0), other.anchor),
                "{:?} and {:?} overlap",
                one.feature,
                other.feature
            );
        }
    }
}

// When two labels collide the higher-priority one is the one that stays, and nothing is
// nudged aside to make room — so a city never loses its label to the tavern inside it.
#[test]
fn the_lower_priority_label_is_the_one_dropped() {
    let candidates = [candidate(1, 0.0, 0.0, 10), candidate(2, 1.0, 0.0, 900)];
    let boxes = [box_of(40.0, 14.0), box_of(40.0, 14.0)];

    let placed = label::place(&candidates, &boxes, 1.0, MAX_LABELS);
    assert_eq!(placed.len(), 1);
    assert_eq!(placed[0].feature, FeatureId(2), "the higher priority survives");
}

// Every sibling POI inside one settlement has the same kind and the same depth, so they
// all tie. A stable sort would hand that tie back to whatever order the document held them
// in, making which label survives depend on the ids that happened to be free when the GM
// drew them — so the tie breaks on FeatureId instead.
#[test]
fn ties_break_the_same_way_whatever_order_the_candidates_arrive_in() {
    let mut forwards = vec![
        candidate(7, 0.0, 0.0, 100),
        candidate(3, 1.0, 0.0, 100),
        candidate(5, 2.0, 0.0, 100),
    ];
    let boxes = [box_of(40.0, 14.0); 3];
    let first = label::place(&forwards, &boxes, 1.0, MAX_LABELS);

    forwards.reverse();
    let boxes_reversed = [box_of(40.0, 14.0); 3];
    let second = label::place(&forwards, &boxes_reversed, 1.0, MAX_LABELS);

    assert_eq!(first.len(), 1);
    assert_eq!(first[0].feature, FeatureId(3), "the lowest id wins a tie");
    assert_eq!(
        first.iter().map(|placed| placed.feature).collect::<Vec<_>>(),
        second.iter().map(|placed| placed.feature).collect::<Vec<_>>(),
    );
}

// The cap is what makes a document of thousands of features cost a bounded label pass
// rather than a quadratic one. The candidates are spaced far enough apart that collision
// drops none of them, so the cap is the only thing that can bound the answer.
#[test]
fn no_more_than_the_cap_is_ever_placed() {
    let mut candidates = Vec::new();
    let mut boxes = Vec::new();
    for id in 0..2_000u64 {
        candidates.push(candidate(id, id as f32 * 500.0, 0.0, 100));
        boxes.push(box_of(20.0, 14.0));
    }

    let placed = label::place(&candidates, &boxes, 1.0, MAX_LABELS);
    assert_eq!(placed.len(), MAX_LABELS);
}

// The box has to over-estimate rather than under-estimate: an error on the high side drops
// a label that would have fit, and an error on the low side lets two labels overlap, which
// is the one thing the pass exists to prevent. It can only be checked against itself —
// bevy's font tables are not reachable from this crate — so what the name pins is the
// direction.
#[test]
fn a_label_box_grows_with_the_text_and_errs_wide() {
    let short = LabelBox::of("Eel", 10.0);
    let long = LabelBox::of("Riverford-on-Eel", 10.0);

    assert!(long.width_pixels > short.width_pixels);
    assert!(
        short.width_pixels >= 3.0 * 10.0,
        "three characters at ten pixels must not be measured under thirty pixels wide"
    );
    assert!(
        short.height_pixels > 10.0,
        "a line is taller than its cap height: it carries a descender and its leading"
    );

    let two_lines = LabelBox::of("Riverford\nEel", 10.0);
    assert!(two_lines.height_pixels > short.height_pixels);
    assert!(
        two_lines.width_pixels < long.width_pixels,
        "a box is as wide as its widest line, not as wide as its characters laid end to end"
    );

    let empty = LabelBox::of("", 10.0);
    assert_eq!(empty.width_pixels, 0.0);
}

// A polygon's label must land inside the polygon. For a crescent the area centroid falls
// in the bay, so the fallback is what keeps the label on the shape it names.
#[test]
fn a_polygons_label_lands_inside_it_even_when_it_is_not_convex() {
    let crescent = Geometry::Polygon(vec![
        at(0.0, 0.0),
        at(10.0, 0.0),
        at(10.0, 10.0),
        at(6.0, 2.0),
        at(4.0, 2.0),
        at(0.0, 10.0),
    ]);
    let anchor = label::anchor(&crescent, 0.0);
    assert!(
        campaign::pick::encloses(crescent.vertices(), anchor),
        "the label landed outside the shape at {anchor}"
    );

    let square = Geometry::Polygon(vec![at(0.0, 0.0), at(4.0, 0.0), at(4.0, 4.0), at(0.0, 4.0)]);
    let middle = label::anchor(&square, 0.0);
    assert!((middle.x - 2.0).abs() < 0.01 && (middle.y - 2.0).abs() < 0.01);
}

// A polyline's label sits half its own length along it rather than at the middle of its
// bounding box, so a road that doubles back does not get its label out in open ground. The
// fixture is 100 cells long, so its midpoint is 40 cells up the second segment.
#[test]
fn a_polylines_label_sits_halfway_along_its_length() {
    let road = Geometry::Polyline(vec![at(0.0, 0.0), at(10.0, 0.0), at(10.0, 90.0)]);
    let anchor = label::anchor(&road, 0.0);

    assert!((anchor.x - 10.0).abs() < 0.01, "{anchor}");
    assert!((anchor.y - 40.0).abs() < 0.01, "{anchor}");
}

// A point's label is lifted clear of the icon drawn at that point. Cell rows run downwards,
// so lifting a label subtracts — getting the sign wrong puts every label under its icon.
#[test]
fn a_points_label_is_lifted_above_its_icon() {
    let point = Geometry::Point(at(5.0, 5.0));
    let anchor = label::anchor(&point, 2.0);
    assert_eq!(anchor.x, 5.0);
    assert_eq!(anchor.y, 3.0, "a lifted label sits at a smaller cell row, not a larger one");
}

// Kind dominates and depth only separates equals, so a capital inside a kingdom still
// outranks a tavern inside that capital.
#[test]
fn kind_outweighs_depth_in_a_labels_priority() {
    let city_inside_a_kingdom = label::priority(100, 1);
    let tavern_inside_the_city = label::priority(20, 2);
    assert!(city_inside_a_kingdom > tavern_inside_the_city);

    let shallow = label::priority(50, 0);
    let deep = label::priority(50, 3);
    assert!(shallow > deep, "at equal kinds, the shallower one wins");
}

// A scale of zero reaches here from a camera that has not been framed, and dividing by it
// would put every anchor at infinity and make every box overlap every other.
#[test]
fn an_unusable_scale_places_nothing_rather_than_everything_at_once() {
    let candidates = [candidate(1, 0.0, 0.0, 10)];
    let boxes = [box_of(40.0, 14.0)];
    for scale in [0.0, -1.0, f32::NAN] {
        assert!(label::place(&candidates, &boxes, scale, MAX_LABELS).is_empty());
    }
}
