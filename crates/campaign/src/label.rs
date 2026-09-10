//! Where a label sits, what it outranks, and which labels fit without overlapping.
//!
//! Automatic placement only. A feature's geometry decides its anchor — a point sits above
//! its icon, a polygon takes an interior point, a polyline its arc-length midpoint — and
//! the GM cannot nudge one. That is a v1 decision rather than a permanent one: nothing
//! here is stored in a document, so a per-feature offset can be added later without
//! changing a single saved world.
//!
//! Placement is a function rather than a system so that "no two labels overlap" is
//! checkable without a window. It takes boxes that have already been measured, because
//! the only thing that can measure a glyph is the renderer, and asking it to would drag a
//! font into the crate that must not need one.

use crate::feature::{CellPoint, FeatureId, Geometry};
use crate::measure::distance;

/// The most labels drawn in one frame.
///
/// A cap rather than a budget that degrades: past this many, the map is unreadable
/// anyway, and the point of the cap is that a document holding thousands of features
/// costs a bounded label pass rather than a quadratic one.
pub const MAX_LABELS: usize = 256;

/// How far above its icon a point feature's label sits, in logical pixels.
pub const LABEL_LIFT_PIXELS: f32 = 9.0;

/// How much of a label's priority one step down the parent chain costs.
///
/// Far smaller than the gap between two kinds, so a capital inside a kingdom still
/// outranks a tavern inside that capital. Depth breaks ties between features of the same
/// kind at different depths, and decides nothing else.
pub const KIND_PRIORITY_STEP: u32 = 16;

/// A label offered for placement.
///
/// `fade` is the feature's detail, carried through unchanged so that a label cannot
/// outlive or out-solid the shape it names.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LabelCandidate {
    pub feature: FeatureId,
    pub anchor: CellPoint,
    pub priority: u32,
    pub fade: f32,
}

/// How much room a label needs, in logical pixels.
///
/// Over-estimated rather than under-estimated, always: an error in the box then drops a
/// label that would have fit, which is a label the GM does not see. Under-estimating
/// would let two labels overlap, which is the one outcome the whole pass exists to
/// prevent.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LabelBox {
    pub width_pixels: f32,
    pub height_pixels: f32,
}

const WIDEST_ADVANCE: f32 = 1.3;
const LINE_HEIGHT: f32 = 1.8;

impl LabelBox {
    /// Room enough for `text` drawn at `font_pixels`, and then some.
    ///
    /// Counts every character at the widest advance bevy's Hershey simplex font has, so
    /// the answer is an over-estimate for any real string and exact only for one made
    /// entirely of the widest glyph. Characters that font cannot draw are counted too —
    /// they cost nothing on screen, and counting them keeps this on the safe side.
    pub fn of(text: &str, font_pixels: f32) -> Self {
        let widest = text.lines().map(|line| line.chars().count()).max().unwrap_or(0);
        let lines = text.lines().count().max(1);
        Self {
            width_pixels: widest as f32 * font_pixels * WIDEST_ADVANCE,
            height_pixels: lines as f32 * font_pixels * LINE_HEIGHT,
        }
    }

    /// Whether this box, centred at `here`, meets `other` centred at `there`.
    ///
    /// Touching exactly counts as meeting: two labels sharing an edge are two labels the
    /// eye reads as one.
    pub fn meets(self, here: CellPoint, other: Self, there: CellPoint) -> bool {
        let gap_x = (here.x - there.x).abs();
        let gap_y = (here.y - there.y).abs();
        gap_x * 2.0 <= self.width_pixels + other.width_pixels
            && gap_y * 2.0 <= self.height_pixels + other.height_pixels
    }
}

/// A label that was placed, and how solidly to draw it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Placed {
    pub feature: FeatureId,
    pub anchor: CellPoint,
    pub fade: f32,
}

/// How strongly a feature's label competes, from its kind's priority and its depth in the
/// parent chain.
///
/// Kind dominates and depth only separates equals: a city inside a kingdom outranks a
/// tavern inside that city because it is a settlement, not because of where it sits.
pub fn priority(kind_priority: u32, depth: usize) -> u32 {
    kind_priority
        .saturating_mul(KIND_PRIORITY_STEP)
        .saturating_sub(depth.min(KIND_PRIORITY_STEP as usize) as u32)
}

/// Where a label for `geometry` sits, in terrain cells.
///
/// `lift_cells` is how far above the anchor a **point**'s label is raised, so that it
/// clears the icon drawn there; it is in cells because that is what the caller has, and
/// the renderer converts its own pixel constant once. Cell rows run downwards, so lifting
/// a label subtracts.
///
/// - A point takes its own position, lifted.
/// - A polygon takes an interior point: its area centroid where that falls inside the
///   ring, and otherwise the middle of the widest run of inside-ness along the centroid's
///   own row. A crescent's label then sits in the crescent rather than in the bay.
/// - A polyline takes the point half its own length along it, which is the middle of the
///   drawn line rather than the middle of its bounding box.
pub fn anchor(geometry: &Geometry, lift_cells: f32) -> CellPoint {
    match geometry {
        Geometry::Point(at) => CellPoint::new(at.x, at.y - lift_cells),
        Geometry::Polygon(vertices) => interior_point(vertices),
        Geometry::Polyline(vertices) => along(vertices),
    }
}

/// Which candidates are drawn, highest priority first.
///
/// A candidate whose box meets the box of one already placed is dropped — the
/// higher-priority label is the one that stays, and nothing is nudged aside to make room.
/// At most [`MAX_LABELS`] are placed however many are offered.
///
/// `boxes` runs alongside `candidates`; a candidate with no box is dropped rather than
/// guessed at. `cells_per_pixel` converts the anchors into the pixel space the boxes are
/// measured in — only relative positions matter, so the camera's own position and the
/// world's y flip are not needed and not asked for.
///
/// Ties on priority break on [`FeatureId`], not on the order the candidates arrive in:
/// every sibling POI inside one settlement has the same kind and the same depth and so
/// the same priority, and a stable sort would hand that tie back to whatever order the
/// document happened to hold them in — making which label survives depend on the ids that
/// were free when the GM drew them.
pub fn place(
    candidates: &[LabelCandidate],
    boxes: &[LabelBox],
    cells_per_pixel: f32,
    max: usize,
) -> Vec<Placed> {
    if !cells_per_pixel.is_finite() || cells_per_pixel <= 0.0 {
        return Vec::new();
    }

    let mut order: Vec<usize> = (0..candidates.len().min(boxes.len())).collect();
    order.sort_by(|a, b| {
        candidates[*b]
            .priority
            .cmp(&candidates[*a].priority)
            .then(candidates[*a].feature.cmp(&candidates[*b].feature))
    });

    let mut placed: Vec<Placed> = Vec::new();
    let mut taken: Vec<(CellPoint, LabelBox)> = Vec::new();

    for index in order {
        if placed.len() >= max {
            break;
        }
        let candidate = candidates[index];
        let extent = boxes[index];
        let at = CellPoint::new(
            candidate.anchor.x / cells_per_pixel,
            candidate.anchor.y / cells_per_pixel,
        );
        if taken
            .iter()
            .any(|(there, other)| extent.meets(at, *other, *there))
        {
            continue;
        }
        taken.push((at, extent));
        placed.push(Placed {
            feature: candidate.feature,
            anchor: candidate.anchor,
            fade: candidate.fade,
        });
    }

    placed
}

fn along(vertices: &[CellPoint]) -> CellPoint {
    let Some(first) = vertices.first().copied() else {
        return CellPoint::new(0.0, 0.0);
    };
    let total: f32 = vertices
        .windows(2)
        .map(|pair| distance(pair[0], pair[1]))
        .sum();
    if total <= 0.0 {
        return first;
    }

    let mut walked = 0.0;
    for pair in vertices.windows(2) {
        let step = distance(pair[0], pair[1]);
        if walked + step >= total / 2.0 && step > 0.0 {
            let into = (total / 2.0 - walked) / step;
            return CellPoint::new(
                pair[0].x + (pair[1].x - pair[0].x) * into,
                pair[0].y + (pair[1].y - pair[0].y) * into,
            );
        }
        walked += step;
    }
    first
}

fn interior_point(vertices: &[CellPoint]) -> CellPoint {
    let centre = centroid(vertices);
    if crate::pick::encloses(vertices, centre) {
        return centre;
    }
    widest_span(vertices, centre.y).unwrap_or(centre)
}

fn centroid(vertices: &[CellPoint]) -> CellPoint {
    if vertices.len() < 3 {
        return mean(vertices);
    }
    let mut twice_area = 0.0;
    let mut x = 0.0;
    let mut y = 0.0;
    for current in 0..vertices.len() {
        let a = vertices[current];
        let b = vertices[(current + 1) % vertices.len()];
        let cross = a.x * b.y - b.x * a.y;
        twice_area += cross;
        x += (a.x + b.x) * cross;
        y += (a.y + b.y) * cross;
    }
    if twice_area.abs() <= f32::EPSILON {
        return mean(vertices);
    }
    CellPoint::new(x / (3.0 * twice_area), y / (3.0 * twice_area))
}

fn mean(vertices: &[CellPoint]) -> CellPoint {
    if vertices.is_empty() {
        return CellPoint::new(0.0, 0.0);
    }
    let count = vertices.len() as f32;
    let x = vertices.iter().map(|vertex| vertex.x).sum::<f32>() / count;
    let y = vertices.iter().map(|vertex| vertex.y).sum::<f32>() / count;
    CellPoint::new(x, y)
}

fn widest_span(vertices: &[CellPoint], row: f32) -> Option<CellPoint> {
    let mut crossings: Vec<f32> = Vec::new();
    for current in 0..vertices.len() {
        let a = vertices[current];
        let b = vertices[(current + 1) % vertices.len()];
        if (a.y > row) == (b.y > row) {
            continue;
        }
        let into = (row - a.y) / (b.y - a.y);
        crossings.push(a.x + (b.x - a.x) * into);
    }
    crossings.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

    crossings
        .as_chunks::<2>()
        .0
        .iter()
        .map(|span| (span[1] - span[0], (span[0] + span[1]) / 2.0))
        .max_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal))
        .map(|(_, middle)| CellPoint::new(middle, row))
}

