//! What one cell of a document is worth, and every figure derived from it that a GM reads.
//!
//! One module answers "how far is that", so a distance typed into an image calibration and
//! a distance measured with the ruler cannot disagree. The editor asks here and formats
//! nothing of its own: a rule that needs a window to check is a rule that will not be
//! checked.
//!
//! Two number widths, and the split is deliberate. A **cell** is `f32`, because that is
//! what a [`CellPoint`] holds and geometry is compared against geometry. A **unit** is
//! `f64`, because it is multiplied by a scale the GM chose and then divided by a zoom, and
//! the two ends of that range are further apart than `f32` keeps digits over.
//! [`CellWorth::units`] is the one place the widths meet.

use crate::feature::{CellPoint, Geometry};
use crate::manifest::CampaignManifest;
use crate::world::World;

/// Metres one foot spans.
///
/// Exact by definition, which its reciprocal is not — so feet are converted by dividing by
/// this rather than by multiplying by 3.28084.
pub const METRES_PER_FOOT: f64 = 0.3048;

/// A unit the scale bar may step through and the campaign panel may offer.
///
/// A closed set with a factor and a family, not a list of labels: the bar changes unit as
/// the map is zoomed, which needs both to know what to step to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DistanceUnit {
    Feet,
    Miles,
    Leagues,
    Metres,
    Kilometres,
}

/// The units a scale bar may step between.
///
/// A step only ever lands inside one family, so a campaign in miles never reads in metres.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnitFamily {
    Imperial,
    Metric,
}

impl DistanceUnit {
    /// Every unit, smallest first within each family.
    pub fn all() -> [Self; 5] {
        [
            Self::Feet,
            Self::Miles,
            Self::Leagues,
            Self::Metres,
            Self::Kilometres,
        ]
    }

    /// How this unit is written in `campaign.ron` and on screen.
    pub fn label(self) -> &'static str {
        match self {
            Self::Feet => "ft",
            Self::Miles => "miles",
            Self::Leagues => "leagues",
            Self::Metres => "m",
            Self::Kilometres => "km",
        }
    }

    /// The unit written as `label`, or `None` for a label this build does not know.
    ///
    /// A campaign is free to carry any label at all, so an unknown one is an ordinary
    /// answer here rather than a fault: it means the bar cannot step and keeps that label
    /// at every zoom.
    pub fn from_label(label: &str) -> Option<Self> {
        Self::all()
            .into_iter()
            .find(|unit| unit.label().eq_ignore_ascii_case(label))
    }

    /// Metres one of this unit spans.
    pub fn metres(self) -> f64 {
        match self {
            Self::Feet => METRES_PER_FOOT,
            Self::Miles => 1609.344,
            Self::Leagues => 4828.032,
            Self::Metres => 1.0,
            Self::Kilometres => 1000.0,
        }
    }

    /// Which family this unit steps within.
    pub fn family(self) -> UnitFamily {
        match self {
            Self::Feet | Self::Miles | Self::Leagues => UnitFamily::Imperial,
            Self::Metres | Self::Kilometres => UnitFamily::Metric,
        }
    }
}

/// What one cell of one document is worth, and whether a pace means anything on it.
///
/// Built by [`worth_of`] and nowhere else, so every consumer is given the same answer for
/// the same document.
#[derive(Debug, Clone, PartialEq)]
pub struct CellWorth {
    units_per_cell: f64,
    unit: String,
    travelled: bool,
}

impl CellWorth {
    /// How many of [`CellWorth::unit`] one cell of this document spans. Finite and above
    /// zero.
    pub fn units_per_cell(&self) -> f64 {
        self.units_per_cell
    }

    /// The unit every figure for this document is written in.
    pub fn unit(&self) -> &str {
        &self.unit
    }

    /// That unit as a [`DistanceUnit`], or `None` when the campaign carries a label this
    /// build does not know.
    pub fn known_unit(&self) -> Option<DistanceUnit> {
        DistanceUnit::from_label(&self.unit)
    }

    /// Whether a distance on this document may be turned into a travel time.
    ///
    /// False inside a dungeon: a pace in days says nothing about a corridor.
    pub fn travelled(&self) -> bool {
        self.travelled
    }

    /// `cells` of this document as a figure in [`CellWorth::unit`].
    ///
    /// The one place a cell count becomes a distance the GM reads, and so the one place
    /// the `f32` of geometry becomes the `f64` of a figure.
    pub fn units(&self, cells: f32) -> f64 {
        f64::from(cells) * self.units_per_cell
    }
}

/// Why `units_per_cell` is not a worth a document may be measured at, or `None` if it is
/// fine.
///
/// Bounded above as well as below, as [`image::scale_refusal`](crate::image::scale_refusal)
/// is: this number is divided into a zoom to size the scale bar, and a finite but enormous
/// worth makes that quotient infinite.
pub fn worth_refusal(units_per_cell: f64) -> Option<&'static str> {
    if !units_per_cell.is_finite() {
        return Some("is not a finite number");
    }
    if units_per_cell <= 0.0 {
        return Some("is not greater than zero");
    }
    if units_per_cell < crate::manifest::MIN_UNITS_PER_CELL {
        return Some("is small enough that no round figure fits on a scale bar");
    }
    if units_per_cell > crate::manifest::MAX_UNITS_PER_CELL {
        return Some("is large enough to put every figure derived from it beyond counting");
    }
    None
}

/// What one cell of `world` is worth, given the campaign it belongs to.
///
/// A document carrying a [`TileGrid`](crate::grid::TileGrid) is a dungeon and answers in
/// feet off that grid, whatever the campaign's own unit is; every other document answers
/// with the campaign's `units_per_cell` and unit. A dungeon also answers that it is not
/// travelled, so no figure inside one is ever a duration.
pub fn worth_of(world: &World, manifest: &CampaignManifest) -> CellWorth {
    match world.grid() {
        Some(grid) => CellWorth {
            units_per_cell: f64::from(grid.metres_per_cell()) / METRES_PER_FOOT,
            unit: DistanceUnit::Feet.label().to_owned(),
            travelled: false,
        },
        None => CellWorth {
            units_per_cell: manifest.units_per_cell,
            unit: manifest.unit.clone(),
            travelled: true,
        },
    }
}

/// How far apart two cell positions are, in cells.
pub fn distance(from: CellPoint, to: CellPoint) -> f32 {
    ((to.x - from.x).powi(2) + (to.y - from.y).powi(2)).sqrt()
}

/// How long a path through `vertices` is, in cells.
///
/// `closed` adds the edge from the last vertex back to the first, which is what a polygon's
/// perimeter needs — a polygon does not store that edge, so a sum over its stored vertices
/// alone is one edge short. Fewer than two vertices is no path and answers zero.
pub fn path_length(vertices: &[CellPoint], closed: bool) -> f32 {
    if vertices.len() < 2 {
        return 0.0;
    }
    let open: f32 = vertices
        .windows(2)
        .map(|pair| distance(pair[0], pair[1]))
        .sum();
    match closed {
        true => open + distance(vertices[vertices.len() - 1], vertices[0]),
        false => open,
    }
}

/// Whether a geometry's length is measured all the way round.
pub fn closes(geometry: &Geometry) -> bool {
    matches!(geometry, Geometry::Polygon(_))
}

/// One distance and how long it takes to travel.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Leg {
    /// The distance, in the document's own unit.
    pub distance: f64,
    /// Days to travel it at the pace given, or `None` where a pace does not apply.
    pub days: Option<f64>,
}

/// What a path measures: its own length, and the straight line across it where that is a
/// different question.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Measurement {
    /// The length along the path itself.
    pub path: Leg,
    /// The line from the path's first point to its last.
    ///
    /// `None` for a closed path, where first-to-last is one edge rather than the distance
    /// avoided by not following the path.
    pub straight: Option<Leg>,
}

/// What `points` measures on a document worth `worth`, at `pace` units a day.
///
/// `None` for fewer than two points, which is not yet a measurement. `pace` is ignored on
/// a document [`CellWorth::travelled`] calls false, so a dungeon answers distances with no
/// durations beside them.
pub fn measure_of(
    points: &[CellPoint],
    closed: bool,
    worth: &CellWorth,
    pace: f64,
) -> Option<Measurement> {
    if points.len() < 2 {
        return None;
    }
    let days = |distance: f64| match worth.travelled() && pace.is_finite() && pace > 0.0 {
        true => Some(distance / pace),
        false => None,
    };
    let path = worth.units(path_length(points, closed));
    let straight = worth.units(distance(points[0], points[points.len() - 1]));

    Some(Measurement {
        path: Leg {
            distance: path,
            days: days(path),
        },
        straight: match closed {
            true => None,
            false => Some(Leg {
                distance: straight,
                days: days(straight),
            }),
        },
    })
}

/// The bar to draw for a map at `units_per_pixel`, no wider than `max_pixels`.
///
/// Answers the figure to label it with, the unit that figure reads best in, and how many
/// logical pixels long the bar is. The figure is 1, 2 or 5 times a power of ten.
///
/// The unit steps with the zoom: a figure below one in the campaign's own unit steps down
/// to a smaller unit of the same family, and one grown large steps up, so a campaign in
/// kilometres zoomed into a city reads in metres. A campaign whose unit this build does not
/// know cannot step and keeps its own label at every zoom.
///
/// `None` when `units_per_pixel` or `max_pixels` is not finite and above zero, which is a
/// map with no scale to draw rather than a fault.
pub fn scale_bar(
    units_per_pixel: f64,
    worth: &CellWorth,
    max_pixels: f32,
) -> Option<(f64, String, f32)> {
    if !units_per_pixel.is_finite() || units_per_pixel <= 0.0 {
        return None;
    }
    if !max_pixels.is_finite() || max_pixels <= 0.0 {
        return None;
    }
    let cap = units_per_pixel * f64::from(max_pixels);
    if !cap.is_finite() || cap <= 0.0 {
        return None;
    }

    let (figure, label, in_campaign_units) = match worth.known_unit() {
        Some(own) => {
            let stepped = step_to(cap, own);
            let figure = round_figure(cap * own.metres() / stepped.metres())?;
            (
                figure,
                stepped.label().to_owned(),
                figure * stepped.metres() / own.metres(),
            )
        }
        None => {
            let figure = round_figure(cap)?;
            (figure, worth.unit().to_owned(), figure)
        }
    };

    let pixels = in_campaign_units / units_per_pixel;
    match pixels.is_finite() && pixels > 0.0 {
        true => Some((figure, label, pixels as f32)),
        false => None,
    }
}

/// `value` written the way a GM reads it, with its unit.
///
/// The one formatter, so the scale bar, the ruler and the selection panel cannot drift
/// apart in how they round.
pub fn figure(value: f64, unit: &str) -> String {
    let places = match value.abs() {
        v if v >= 100.0 => 0,
        v if v >= 10.0 => 1,
        _ => 2,
    };
    format!("{value:.places$} {unit}", places = places)
}

/// `days` written the way a GM reads it.
pub fn duration(days: f64) -> String {
    match days {
        d if d < 1.0 => format!("{:.1} hours", d * 24.0),
        d => format!("{d:.1} days"),
    }
}

fn step_to(cap: f64, own: DistanceUnit) -> DistanceUnit {
    let family = own.family();
    let metres = cap * own.metres();
    let mut candidates: Vec<DistanceUnit> = DistanceUnit::all()
        .into_iter()
        .filter(|unit| unit.family() == family)
        .collect();
    candidates.sort_by(|a, b| {
        a.metres()
            .partial_cmp(&b.metres())
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let mut chosen = candidates[0];
    for candidate in candidates {
        if metres / candidate.metres() >= 1.0 {
            chosen = candidate;
        }
    }
    chosen
}

fn round_figure(cap: f64) -> Option<f64> {
    if !cap.is_finite() || cap <= 0.0 {
        return None;
    }
    let exponent = cap.log10().floor();
    if !exponent.is_finite() {
        return None;
    }
    let base = 10f64.powf(exponent);
    if !base.is_finite() || base <= 0.0 {
        return None;
    }
    for step in [5.0, 2.0, 1.0] {
        let figure = step * base;
        if figure <= cap && figure > 0.0 {
            return Some(figure);
        }
    }
    None
}
