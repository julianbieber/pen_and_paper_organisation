//! How much detail a feature is drawn at, for a given map scale.
//!
//! One function, and it is the only place either reveal threshold is decided. Its answer
//! drives the skip, the alpha and the label fade alike, so a feature that is drawn is
//! never drawn at an alpha of zero, and — because the hit tests ask the same function —
//! a press cannot land on something the same frame declined to draw.
//!
//! It is a function rather than a system for the reason everything in this crate is:
//! whether a tavern is visible at a given zoom is a rule, and a rule that needs a window
//! to check is a rule that will not be checked.
//!
//! Both thresholds fade rather than step. A hamlet and a capital then reveal themselves
//! at the scale their own polygons deserve, with no per-feature tuning and nothing
//! blinking into existence as the camera drifts.

use std::collections::BTreeMap;

use crate::feature::{FeatureId, Geometry};
use crate::world::World;

/// The on-screen area, in square logical pixels, at which a parent begins to reveal what
/// is inside it.
///
/// Small enough that a settlement drawn at a plausible size on a world map opens up
/// before it fills the view. A kind that covers far more ground than a settlement — a
/// kingdom, a forest — reveals its children almost immediately at this figure, which is
/// what [`crate::feature::Feature::max_cells_per_pixel`] is for.
pub const DETAIL_AREA_SQUARE_PIXELS: f32 = 12_000.0;

/// How far past [`DETAIL_AREA_SQUARE_PIXELS`] a parent has to grow before what is inside
/// it is drawn at full strength.
///
/// The span is what makes the reveal a fade rather than a step, and a fade is what keeps
/// a slow zoom from flickering as a polygon's area crosses a single number.
pub const DETAIL_FADE_SPAN: f32 = 2.5;

/// How far past its own `max_cells_per_pixel` a feature fades out over, as a multiple of
/// that threshold.
///
/// Its own constant rather than [`DETAIL_FADE_SPAN`]: one is a span in area and the other
/// a span in scale, and they are only coincidentally similar numbers.
pub const ZOOM_FADE_SPAN: f32 = 1.6;

/// The least detail at which a feature can still be clicked.
///
/// A floor rather than "anything above zero", because a feature drawn at two percent
/// alpha is not something the GM can see to aim at, and letting a press land on it makes
/// a click on apparently empty ground select something invisible.
pub const MIN_PICKABLE_DETAIL: f32 = 0.15;

/// Every polygon's area in square terrain cells, kept beside the document.
///
/// Cached because an area is a fact about the document that the camera cannot alter, and
/// recomputing the shoelace sum of every polygon each frame would make the cost of the
/// draw pass depend on how much has been drawn rather than on what is on screen.
///
/// A feature with no entry constrains nothing: an absent area and a zero area are treated
/// the same way, so a cache that has not caught up hides nothing.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Areas {
    cells: BTreeMap<FeatureId, f32>,
}

impl Areas {
    /// The areas of every polygon in `world`.
    ///
    /// Only polygons: a point and a polyline enclose nothing, and an entry of zero for
    /// them would be indistinguishable from a degenerate polygon.
    pub fn of(world: &World) -> Self {
        let cells = world
            .features()
            .filter(|(_, feature)| matches!(feature.geometry, Geometry::Polygon(_)))
            .map(|(id, feature)| (id, crate::pick::area(feature.geometry.vertices())))
            .collect();
        Self { cells }
    }

    /// Rebuild this cache from `world`, reusing the allocation.
    ///
    /// Clears rather than updating in place: an edit can delete a feature as easily as
    /// move a vertex, and a table that only ever gained entries would answer for
    /// features the document no longer holds.
    pub fn rebuild(&mut self, world: &World) {
        self.cells.clear();
        for (id, feature) in world.features() {
            if matches!(feature.geometry, Geometry::Polygon(_)) {
                self.cells
                    .insert(id, crate::pick::area(feature.geometry.vertices()));
            }
        }
    }

    /// The area `id` encloses in square terrain cells, or zero for anything that encloses
    /// nothing or is not in the cache.
    ///
    /// The whole shoelace area rather than the part on screen: detail then rises with
    /// zoom and never changes under a pan.
    pub fn of_feature(&self, id: FeatureId) -> f32 {
        self.cells.get(&id).copied().unwrap_or(0.0)
    }

    /// How many polygons this cache answers for.
    pub fn len(&self) -> usize {
        self.cells.len()
    }

    /// Whether this cache answers for nothing, which is what an empty world produces.
    pub fn is_empty(&self) -> bool {
        self.cells.is_empty()
    }
}

/// How much of `id` is drawn at a map scale of `cells_per_pixel`, from zero to one.
///
/// Zero means not drawn at all; one means drawn in full. Anything between is an alpha,
/// and the same number is the label's fade — one answer, so a label cannot outlive the
/// shape it names.
///
/// `cells_per_pixel` is in terrain cells per **logical** pixel, which is the currency
/// every threshold in this module is written in.
///
/// `exempt` is the selection. A selected feature answers one whatever the scale, because
/// a feature that faded out of its own selection could not be clicked to deselect. The
/// exemption lives inside this function rather than at the two call sites so that drawing
/// and picking agree by construction.
///
/// An id the world does not hold answers zero: there is nothing to draw.
pub fn detail(world: &World, areas: &Areas, cells_per_pixel: f32, id: FeatureId, exempt: &[FeatureId]) -> f32 {
    if exempt.contains(&id) {
        return 1.0;
    }
    let Some(feature) = world.feature(id) else {
        return 0.0;
    };
    if !cells_per_pixel.is_finite() || cells_per_pixel <= 0.0 {
        return 0.0;
    }

    let mut least = own_factor(feature.max_cells_per_pixel, cells_per_pixel);
    if least <= 0.0 {
        return 0.0;
    }

    let mut current = feature.parent;
    let mut walked = 0usize;
    while let Some(ancestor) = current {
        walked += 1;
        if walked > world.len() {
            break;
        }
        let Some(parent) = world.feature(ancestor) else {
            break;
        };
        if matches!(parent.geometry, Geometry::Polygon(_)) {
            let area = areas.of_feature(ancestor);
            if area > 0.0 {
                least = least.min(area_factor(area, cells_per_pixel));
                if least <= 0.0 {
                    return 0.0;
                }
            }
        }
        current = parent.parent;
    }

    least
}

/// Whether `id` is drawn solidly enough at `cells_per_pixel` to be worth aiming at.
///
/// What every hit test is filtered by, so that what can be picked is what is drawn.
pub fn is_pickable(
    world: &World,
    areas: &Areas,
    cells_per_pixel: f32,
    id: FeatureId,
    exempt: &[FeatureId],
) -> bool {
    detail(world, areas, cells_per_pixel, id, exempt) >= MIN_PICKABLE_DETAIL
}

fn own_factor(max_cells_per_pixel: Option<f32>, cells_per_pixel: f32) -> f32 {
    let Some(threshold) = max_cells_per_pixel.filter(|scale| scale.is_finite() && *scale > 0.0)
    else {
        return 1.0;
    };
    if cells_per_pixel <= threshold {
        return 1.0;
    }
    let coarsest = threshold * ZOOM_FADE_SPAN;
    if cells_per_pixel >= coarsest {
        return 0.0;
    }
    (coarsest - cells_per_pixel) / (coarsest - threshold)
}

fn area_factor(area_cells: f32, cells_per_pixel: f32) -> f32 {
    let square_pixels = area_cells / (cells_per_pixel * cells_per_pixel);
    if !square_pixels.is_finite() {
        return 1.0;
    }
    let full = DETAIL_AREA_SQUARE_PIXELS * DETAIL_FADE_SPAN;
    if square_pixels <= DETAIL_AREA_SQUARE_PIXELS {
        return 0.0;
    }
    if square_pixels >= full {
        return 1.0;
    }
    (square_pixels - DETAIL_AREA_SQUARE_PIXELS) / (full - DETAIL_AREA_SQUARE_PIXELS)
}
