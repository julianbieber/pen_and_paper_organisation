//! What a position in terrain cells lands on, what a rectangle encloses, and where a
//! vertex being placed actually goes.
//!
//! One module rather than three, because all of it is the same question — what is near
//! this position — over the same distance primitives. Split up, each half would grow its
//! own point-to-segment distance, and a distance that disagrees with itself is how a
//! click selects one feature and the drag that follows moves another.
//!
//! Everything here is a pure function over a [`World`] and positions in cells. The editor
//! decides nothing about geometry: it turns a cursor into a cell, asks, and applies the
//! answer.

use crate::feature::{CellPoint, FeatureId, FeatureKind, Geometry};
use crate::world::World;

/// Which part of a feature a position landed on.
///
/// Ordered by how deliberate the aim must be: hitting a vertex takes more precision than
/// an edge, and an edge more than a body. [`pick`] returns the most precise hit available
/// within the slack, so overlapping targets resolve to the small one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hit {
    Vertex,
    Edge,
    Body,
}

/// What [`pick`] found: which feature, which part of it, and where in its vertex list.
///
/// `index` means something different per `hit`, and getting it wrong silently edits the
/// wrong vertex:
///
/// - [`Hit::Vertex`] — the index of that vertex.
/// - [`Hit::Edge`] — the index a new vertex would be **inserted at** to split that edge,
///   which is one past the vertex the edge leaves. Handing this straight to
///   [`Edit::InsertVertex`](crate::edit::Edit::InsertVertex) splits the edge that was
///   clicked.
/// - [`Hit::Body`] — always zero, and meaningless.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Landing {
    pub feature: FeatureId,
    pub hit: Hit,
    pub index: usize,
}

/// Where a vertex being placed lands, and what it landed on.
///
/// `at` is the position to use — the snap target's own position when there was one, and
/// the position asked about unchanged when there was not, so a caller may always use it.
/// `onto` names the feature snapped to, for an overlay to mark before the press.
/// `closes` is true only when the target was the first vertex of the shape being drawn,
/// which is what finishes a polygon.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Snap {
    pub at: CellPoint,
    pub onto: Option<FeatureId>,
    pub closes: bool,
}

impl Snap {
    /// The position unchanged, having found nothing to snap to.
    pub fn none(at: CellPoint) -> Self {
        Self {
            at,
            onto: None,
            closes: false,
        }
    }

    /// Whether this snap moved the position at all.
    pub fn landed(&self) -> bool {
        self.onto.is_some() || self.closes
    }
}

/// What `at` lands on in `world`, within `slack` cells, or nothing.
///
/// Prefers a vertex to an edge to a body, across the whole world rather than per feature:
/// a vertex of one feature beats an edge of another, because the aim was the vertex.
/// Among equals, the nearest wins; among bodies, the smallest, so a tavern's own polygon
/// beats the city it sits in.
///
/// A point has only its vertex, a polyline has vertices and edges, a polygon has all
/// three — so a click in the middle of a polyline answers [`Hit::Edge`], and there is no
/// way for it to answer [`Hit::Body`].
pub fn pick(world: &World, at: CellPoint, slack: f32) -> Option<Landing> {
    let mut vertex: Option<(f32, Landing)> = None;
    let mut edge: Option<(f32, Landing)> = None;
    let mut body: Option<(f32, Landing)> = None;

    for (id, feature) in world.features() {
        let vertices = feature.geometry.vertices();

        for (index, position) in vertices.iter().enumerate() {
            let distance = distance(at, *position);
            if distance <= slack {
                keep(
                    &mut vertex,
                    distance,
                    Landing {
                        feature: id,
                        hit: Hit::Vertex,
                        index,
                    },
                );
            }
        }

        for (segment, (from, to)) in segments(&feature.geometry).enumerate() {
            let distance = distance_to_segment(at, from, to);
            if distance <= slack {
                keep(
                    &mut edge,
                    distance,
                    Landing {
                        feature: id,
                        hit: Hit::Edge,
                        index: segment + 1,
                    },
                );
            }
        }

        if matches!(feature.geometry, Geometry::Polygon(_)) && encloses(vertices, at) {
            keep(
                &mut body,
                area(vertices),
                Landing {
                    feature: id,
                    hit: Hit::Body,
                    index: 0,
                },
            );
        }
    }

    vertex.or(edge).or(body).map(|(_, landing)| landing)
}

/// The smallest settlement whose polygon encloses `at`, or nothing.
///
/// This is what parents a tavern to the city it was placed in. Deliberately settlements
/// only: a POI silently parented to the kingdom it happens to sit in would make deleting
/// that kingdom ask what to do with every POI on the map.
pub fn enclosing_settlement(world: &World, at: CellPoint) -> Option<FeatureId> {
    world
        .features()
        .filter(|(_, feature)| feature.kind == FeatureKind::Settlement)
        .filter(|(_, feature)| matches!(feature.geometry, Geometry::Polygon(_)))
        .filter(|(_, feature)| encloses(feature.geometry.vertices(), at))
        .min_by(|(_, a), (_, b)| {
            area(a.geometry.vertices())
                .partial_cmp(&area(b.geometry.vertices()))
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .map(|(id, _)| id)
}

/// Every feature lying wholly inside the rectangle with `corner` and `opposite` as
/// opposing corners, in id order.
///
/// Wholly: a feature is enclosed when **every** vertex is inside. A polyline crossing the
/// box is therefore not selected — the rule a GM can predict without seeing the geometry,
/// and the one that makes dragging a box over a city not also catch the road passing
/// through it.
pub fn within(world: &World, corner: CellPoint, opposite: CellPoint) -> Vec<FeatureId> {
    let low = CellPoint::new(corner.x.min(opposite.x), corner.y.min(opposite.y));
    let high = CellPoint::new(corner.x.max(opposite.x), corner.y.max(opposite.y));

    world
        .features()
        .filter(|(_, feature)| {
            feature.geometry.vertices().iter().all(|vertex| {
                vertex.x >= low.x && vertex.x <= high.x && vertex.y >= low.y && vertex.y <= high.y
            })
        })
        .map(|(id, _)| id)
        .collect()
}

/// Where a vertex placed at `at` actually lands, given what is already drawn and the
/// shape being drawn.
///
/// Targets are the two cases that come up constantly while authoring: an **endpoint** of
/// an existing polyline — a road meeting a road — and any vertex of a **settlement** — a
/// road meeting a city. A settlement drawn as a point is as much a target as one drawn as
/// a polygon: what makes it a target is its kind, not its shape.
///
/// `drafted` is the shape being drawn, whose **first** vertex is also a target. Snapping
/// onto it sets [`Snap::closes`], which is what finishes a polygon; it is preferred over
/// every other target at equal distance, because closing the shape is what the GM meant.
///
/// Returns `at` unchanged when nothing is within `slack`: snapping never moves a vertex
/// that had no target.
pub fn snap(world: &World, drafted: &[CellPoint], at: CellPoint, slack: f32) -> Snap {
    let mut best = Snap::none(at);
    let mut closest = slack;

    if let Some(first) = drafted.first() {
        let distance = distance(at, *first);
        if distance <= closest {
            closest = distance;
            best = Snap {
                at: *first,
                onto: None,
                closes: true,
            };
        }
    }

    for (id, feature) in world.features() {
        for position in snap_targets(&feature.geometry, feature.kind) {
            let distance = distance(at, position);
            if distance < closest {
                closest = distance;
                best = Snap {
                    at: position,
                    onto: Some(id),
                    closes: false,
                };
            }
        }
    }

    best
}

fn snap_targets(geometry: &Geometry, kind: FeatureKind) -> Vec<CellPoint> {
    let vertices = geometry.vertices();
    if kind == FeatureKind::Settlement {
        return vertices.to_vec();
    }
    match geometry {
        Geometry::Polyline(_) => [vertices.first(), vertices.last()]
            .into_iter()
            .flatten()
            .copied()
            .collect(),
        Geometry::Point(_) | Geometry::Polygon(_) => Vec::new(),
    }
}

fn keep(slot: &mut Option<(f32, Landing)>, score: f32, landing: Landing) {
    if slot.is_none_or(|(best, _)| score < best) {
        *slot = Some((score, landing));
    }
}

fn segments(geometry: &Geometry) -> impl Iterator<Item = (CellPoint, CellPoint)> {
    let vertices = geometry.vertices();
    let closing = matches!(geometry, Geometry::Polygon(_)) && vertices.len() > 2;
    let count = match geometry {
        Geometry::Point(_) => 0,
        _ if closing => vertices.len(),
        _ => vertices.len().saturating_sub(1),
    };
    (0..count).map(move |index| (vertices[index], vertices[(index + 1) % vertices.len()]))
}

fn distance(from: CellPoint, to: CellPoint) -> f32 {
    ((to.x - from.x).powi(2) + (to.y - from.y).powi(2)).sqrt()
}

fn distance_to_segment(at: CellPoint, from: CellPoint, to: CellPoint) -> f32 {
    let (dx, dy) = (to.x - from.x, to.y - from.y);
    let length = dx * dx + dy * dy;
    if length <= f32::EPSILON {
        return distance(at, from);
    }
    let along = (((at.x - from.x) * dx + (at.y - from.y) * dy) / length).clamp(0.0, 1.0);
    distance(at, CellPoint::new(from.x + along * dx, from.y + along * dy))
}

fn encloses(vertices: &[CellPoint], at: CellPoint) -> bool {
    if vertices.len() < 3 {
        return false;
    }
    let mut inside = false;
    let mut previous = vertices.len() - 1;
    for current in 0..vertices.len() {
        let (a, b) = (vertices[current], vertices[previous]);
        if (a.y > at.y) != (b.y > at.y)
            && at.x < (b.x - a.x) * (at.y - a.y) / (b.y - a.y) + a.x
        {
            inside = !inside;
        }
        previous = current;
    }
    inside
}

fn area(vertices: &[CellPoint]) -> f32 {
    if vertices.len() < 3 {
        return 0.0;
    }
    let mut sum = 0.0;
    for current in 0..vertices.len() {
        let a = vertices[current];
        let b = vertices[(current + 1) % vertices.len()];
        sum += a.x * b.y - b.x * a.y;
    }
    (sum / 2.0).abs()
}
