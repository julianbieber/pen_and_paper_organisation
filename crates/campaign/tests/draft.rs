//! What a half-drawn shape admits, and the one thing it must never do: hand back a
//! feature the document would refuse.

use campaign::draft::{Draft, DraftShape};
use campaign::feature::{CellPoint, FeatureKind, Geometry};

fn at(x: f32, y: f32) -> CellPoint {
    CellPoint::new(x, y)
}

// The reason a draft is not a Geometry at all: these states are legal to draw through and
// illegal to store.
#[test]
fn a_draft_holds_what_a_geometry_could_not() {
    let mut draft = Draft::new(FeatureKind::Road, DraftShape::Polyline);
    draft.push(at(0.0, 0.0));

    assert_eq!(draft.len(), 1);
    assert!(!draft.can_finish());
    assert!(draft.finish().is_none());
}

// A draft's minimum is asked of the geometry it becomes, so the two cannot drift apart
// when one of them changes.
#[test]
fn a_drafts_minimum_is_the_geometrys_own() {
    assert_eq!(DraftShape::Point.least(), Geometry::Point(at(0.0, 0.0)).least());
    assert_eq!(DraftShape::Polyline.least(), Geometry::Polyline(Vec::new()).least());
    assert_eq!(DraftShape::Polygon.least(), Geometry::Polygon(Vec::new()).least());
}

// Backspace while drawing, and the finished shape after it — the sequence a GM actually
// performs when they misplace a vertex.
#[test]
fn backspace_takes_the_last_vertex_back_off() {
    let mut draft = Draft::new(FeatureKind::Road, DraftShape::Polyline);
    draft.push(at(0.0, 0.0));
    draft.push(at(5.0, 5.0));
    draft.push(at(9.0, 9.0));

    assert_eq!(draft.pop(), Some(at(9.0, 9.0)));
    assert_eq!(draft.len(), 2);

    let feature = draft.finish().expect("two vertices are enough for a polyline");
    assert_eq!(feature.geometry.vertices(), [at(0.0, 0.0), at(5.0, 5.0)]);
}

// A polygon is implicitly closed, so finishing one must not append its first vertex again
// — a triangle is three vertices, and four would be a degenerate edge.
#[test]
fn finishing_a_polygon_adds_no_closing_vertex() {
    let mut draft = Draft::new(FeatureKind::Territory, DraftShape::Polygon);
    for vertex in [at(0.0, 0.0), at(10.0, 0.0), at(10.0, 10.0)] {
        draft.push(vertex);
    }

    let feature = draft.finish().expect("three vertices are enough for a polygon");
    assert_eq!(feature.geometry.len(), 3);
    assert!(matches!(feature.geometry, Geometry::Polygon(_)));
}

// A non-finite coordinate is refused where it is stored, and a draft is a place it could
// be stored — so it never accepts one and the document never sees it.
#[test]
fn a_draft_never_takes_a_coordinate_the_document_would_refuse() {
    let mut draft = Draft::new(FeatureKind::Road, DraftShape::Polyline);
    draft.push(at(f32::NAN, 0.0));
    draft.push(at(0.0, f32::INFINITY));

    assert!(draft.is_empty());
}
