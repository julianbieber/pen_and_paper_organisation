//! The one table saying what a feature looks like, in numbers nothing has to be looked at
//! to check.
//!
//! Every colour, alpha, width, dash, hatch and icon on the map is decided here and
//! nowhere else. The renderer asks and draws; it holds no palette of its own, so "what
//! colour is a river" has exactly one answer and changing it is a one-line diff rather
//! than a search.
//!
//! A [`Stroke`] names a pen rather than carrying a width, and that is the load-bearing
//! part. A width and a dash belong to a gizmo configuration group, which is a *type* and
//! is fixed at compile time — so the set of strokes is fixed too, and the renderer's
//! match over them is the second place a new one has to be handled. Both fail to compile
//! rather than drawing the new stroke as nothing.

use crate::feature::{FeatureKind, Rank};

/// How wide an icon is drawn, in logical pixels, whatever the zoom.
///
/// A settlement's icon is a symbol rather than a footprint — its polygon already says how
/// much ground it covers — so it stays one size on screen, and only its [`Rank`] changes
/// that size.
pub const ICON_PIXELS: f32 = 10.0;

/// How much larger a rank draws its icon than the rank below it.
pub const RANK_ICON_STEP: f32 = 1.45;

/// The pen a feature's outline is drawn through.
///
/// A name rather than a width: a gizmo's width and dash come from a configuration group,
/// which is registered once per type, so a pen is a compile-time thing and a stroke can
/// only ever name one of a fixed set.
///
/// Deliberately not `#[non_exhaustive]` and deliberately without a fallback anywhere it
/// is matched — a stroke added later must be given a pen and a paint order explicitly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Stroke {
    /// The pen a point feature's symbol is drawn with.
    ///
    /// Heavier than a region's outline rather than lighter: an icon is a target the GM
    /// aims at, a handful of pixels across, and one drawn at a boundary's weight is not
    /// legible against terrain at all.
    Icon,
    Road,
    Trail,
    /// The one pen whose width is rewritten every frame, so that a river reads as
    /// continuous with the water the terrain already draws.
    River,
    Border,
    Region,
}

impl Stroke {
    /// Every pen, in the order they paint: fills and washes first, then the lines drawn
    /// over them.
    ///
    /// Gizmos carry no depth against one another, so this order — and not a z coordinate
    /// — is what decides which stroke covers which. The renderer registers its
    /// configuration groups in exactly this sequence.
    pub const fn all() -> [Self; 6] {
        [
            Self::Region,
            Self::Border,
            Self::Icon,
            Self::Trail,
            Self::Road,
            Self::River,
        ]
    }

    /// How wide this pen draws, in logical pixels.
    ///
    /// Fixed on screen rather than in cells: a road is a road at every zoom, and a width
    /// in cells would make one a hairline zoomed out and a ribbon zoomed in.
    ///
    /// [`Stroke::River`] is the exception the renderer overwrites each frame, and the
    /// number here is what it is drawn at before the first frame has measured anything.
    pub const fn width_pixels(self) -> f32 {
        match self {
            Self::Region => 1.0,
            Self::Icon => 2.0,
            Self::Trail => 1.5,
            Self::Border => 2.0,
            Self::Road => 2.5,
            Self::River => 2.0,
        }
    }

    /// Whether this pen draws solid, dotted or dashed.
    ///
    /// [`Stroke::River`] may not be dashed, and the invariant is not decorative: a dash
    /// period is a multiple of the pen's width, and the river's width is rewritten every
    /// frame from the zoom — so a dashed river would change its dash length as the camera
    /// moved.
    pub const fn dash(self) -> Dash {
        match self {
            Self::Icon | Self::Region | Self::Road | Self::River => Dash::Solid,
            Self::Trail => Dash::Dashed {
                line: 2.0,
                gap: 2.0,
            },
            Self::Border => Dash::Dashed {
                line: 4.0,
                gap: 3.0,
            },
        }
    }
}

/// Whether a pen draws a continuous line, and what the breaks in it look like.
///
/// The lengths are multiples of the pen's own width rather than absolute, so a pen stays
/// recognisable when its width changes and a dash never has to be converted between
/// coordinate systems.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Dash {
    Solid,
    Dotted,
    Dashed { line: f32, gap: f32 },
}

impl Dash {
    /// How long one repeat of this dash is, in multiples of the pen's width.
    ///
    /// Zero for a solid line, which has no repeat. What the renderer needs in order to
    /// know how long a segment must be before a dash is visible in it at all.
    pub fn period(self) -> f32 {
        match self {
            Self::Solid => 0.0,
            Self::Dotted => 2.0,
            Self::Dashed { line, gap } => line + gap,
        }
    }
}

/// How the inside of a region is textured.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Pattern {
    Hatch,
    CrossHatch,
}

/// A texture laid inside a closed polygon.
///
/// `spacing_pixels` is on screen rather than in cells, so a wood reads as a wood at every
/// zoom instead of dissolving into a solid block when the camera pulls back. `angle` is
/// in radians, anticlockwise from the world's x axis.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Fill {
    pub pattern: Pattern,
    pub spacing_pixels: f32,
    pub angle: f32,
}

/// The shape a point feature is drawn as.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum IconShape {
    Dot,
    Ring,
    Star,
    Gate,
}

/// Everything about how one feature is drawn.
///
/// The colour is sRGB in zero to one, kept as three numbers rather than a colour type
/// because this crate is the one that must not need a renderer to be tested.
///
/// `alpha` is what the table asks for at full detail. The renderer scales it by the
/// feature's detail, so a feature fades out rather than being switched off — this field
/// is never the thing that hides a feature.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Style {
    pub red: f32,
    pub green: f32,
    pub blue: f32,
    pub alpha: f32,
    pub stroke: Stroke,
    pub fill: Option<Fill>,
    pub icon: Option<IconShape>,
    /// Which label wins a collision, larger first. Combined with the feature's depth in
    /// the parent chain by [`crate::label`], so this is the kind's share of the answer
    /// and not the whole of it.
    pub label_priority: u32,
}

/// How wide an icon of `rank` is drawn, in logical pixels.
///
/// A feature with no rank is drawn at the base size, which is what every non-settlement
/// point takes.
pub fn icon_pixels(rank: Option<Rank>) -> f32 {
    match rank {
        None | Some(Rank::Hamlet) => ICON_PIXELS,
        Some(Rank::Town) => ICON_PIXELS * RANK_ICON_STEP,
        Some(Rank::City) => ICON_PIXELS * RANK_ICON_STEP * RANK_ICON_STEP,
    }
}

/// How `kind` at `rank` is drawn.
///
/// Total over every pair: every kind answers, with or without a rank, so nothing on the
/// map can be drawn as nothing. The match is exhaustive with no catch-all arm, which is
/// what makes adding a [`FeatureKind`] a compile error here rather than a feature that
/// silently renders invisible.
///
/// A rank on a kind that is not a settlement is not refused and not an error — it is
/// simply not consulted. Which rank a road is has no meaning, and refusing it would put a
/// rule in the document format to protect a table that already answers.
pub fn of(kind: FeatureKind, rank: Option<Rank>) -> Style {
    match kind {
        FeatureKind::Settlement => Style {
            red: 0.93,
            green: 0.85,
            blue: 0.66,
            alpha: 1.0,
            stroke: Stroke::Border,
            fill: None,
            icon: Some(match rank {
                Some(Rank::City) => IconShape::Star,
                Some(Rank::Town) => IconShape::Ring,
                None | Some(Rank::Hamlet) => IconShape::Dot,
            }),
            label_priority: settlement_priority(rank),
        },
        FeatureKind::DungeonEntry => Style {
            red: 0.95,
            green: 0.36,
            blue: 0.36,
            alpha: 1.0,
            stroke: Stroke::Icon,
            fill: None,
            icon: Some(IconShape::Gate),
            label_priority: 55,
        },
        FeatureKind::Poi => Style {
            red: 1.0,
            green: 0.93,
            blue: 0.70,
            alpha: 1.0,
            stroke: Stroke::Icon,
            fill: None,
            icon: Some(IconShape::Dot),
            label_priority: 20,
        },
        FeatureKind::Road => Style {
            red: 0.85,
            green: 0.74,
            blue: 0.52,
            alpha: 1.0,
            stroke: Stroke::Road,
            fill: None,
            icon: None,
            label_priority: 40,
        },
        FeatureKind::River => Style {
            red: 0.42,
            green: 0.62,
            blue: 0.82,
            alpha: 1.0,
            stroke: Stroke::River,
            fill: None,
            icon: None,
            label_priority: 45,
        },
        FeatureKind::Trail => Style {
            red: 0.72,
            green: 0.65,
            blue: 0.50,
            alpha: 0.9,
            stroke: Stroke::Trail,
            fill: None,
            icon: None,
            label_priority: 30,
        },
        FeatureKind::Landcover => Style {
            red: 0.44,
            green: 0.62,
            blue: 0.44,
            alpha: 0.75,
            stroke: Stroke::Region,
            fill: Some(Fill {
                pattern: Pattern::Hatch,
                spacing_pixels: 9.0,
                angle: std::f32::consts::FRAC_PI_4,
            }),
            icon: None,
            label_priority: 35,
        },
        FeatureKind::Territory => Style {
            red: 0.72,
            green: 0.55,
            blue: 0.80,
            alpha: 0.85,
            stroke: Stroke::Border,
            fill: None,
            icon: None,
            label_priority: 65,
        },
    }
}

fn settlement_priority(rank: Option<Rank>) -> u32 {
    match rank {
        Some(Rank::City) => 100,
        Some(Rank::Town) => 90,
        None | Some(Rank::Hamlet) => 80,
    }
}
