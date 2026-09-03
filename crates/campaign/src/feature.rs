//! What the GM draws: a feature's identity, the shape it takes on the map, and which
//! kind of thing it is.
//!
//! One type covers everything authored on a map, because a road, a border and the
//! tavern inside a city differ in their kind and their geometry and in nothing else. A
//! second feature type would be a second set of answers to the same questions.
//!
//! Nothing here changes a feature that is already in a world: the crate-visible accessors
//! reaching a geometry's vertices exist for [`crate::edit`], which is the only thing that
//! may, and for nothing else.

use std::fmt;

use serde::{Deserialize, Serialize};

/// A feature's identity, stable for the life of the document that holds it.
///
/// A place note's frontmatter and a child document both refer to a feature by this, so
/// an id outlives the feature it names: it is never handed out twice and never reused
/// after a delete. [`World::fresh_id`](crate::world::World::fresh_id) is the only
/// source of a new one.
///
/// Serializes as the bare number, so a hand-edited `world.ron` reads `7: (...)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct FeatureId(pub u64);

impl fmt::Display for FeatureId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "feature {}", self.0)
    }
}

/// A position in terrain cells — the one coordinate system a world document uses, so
/// that a feature and a terrain sample agree without a conversion at every call site.
///
/// Fractional, because a road does not run from cell centre to cell centre. Both
/// components are finite in any document that loaded: a coordinate that cannot be
/// compared with itself would make the round-trip guarantee false, so one is refused
/// rather than stored.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct CellPoint {
    pub x: f32,
    pub y: f32,
}

impl CellPoint {
    /// The position at `x`, `y`, in terrain cells. Asserts nothing about either: a
    /// non-finite one is refused where it is stored, not where it is built.
    pub fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }

    /// Whether both components are finite, which is what a world document requires of
    /// every coordinate it holds.
    pub fn is_finite(self) -> bool {
        self.x.is_finite() && self.y.is_finite()
    }
}

impl fmt::Display for CellPoint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "({}, {})", self.x, self.y)
    }
}

/// Where a feature sits, and how many vertices its shape requires.
///
/// The variant fixes the arity — a point has exactly one vertex, a polyline at least
/// two, a polygon at least three — and nothing that breaks it survives:
/// [`World::load`](crate::world::World::load) refuses such a document and no
/// [`Edit`](crate::edit::Edit) reduces a geometry below its minimum.
///
/// A polygon is implicitly closed: the edge from its last vertex back to its first is
/// not stored, so a triangle is three vertices and not four.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Geometry {
    Point(CellPoint),
    Polyline(Vec<CellPoint>),
    Polygon(Vec<CellPoint>),
}

impl Geometry {
    /// This geometry's vertices in order, however many its shape holds.
    pub fn vertices(&self) -> &[CellPoint] {
        match self {
            Self::Point(vertex) => std::slice::from_ref(vertex),
            Self::Polyline(vertices) | Self::Polygon(vertices) => vertices,
        }
    }

    /// How many vertices this geometry holds.
    pub fn len(&self) -> usize {
        self.vertices().len()
    }

    /// Always false — every geometry holds at least one vertex. Present because
    /// [`Geometry::len`] exists.
    pub fn is_empty(&self) -> bool {
        false
    }

    /// The fewest vertices this shape admits: one for a point, two for a polyline,
    /// three for a polygon.
    pub fn least(&self) -> usize {
        match self {
            Self::Point(_) => 1,
            Self::Polyline(_) => 2,
            Self::Polygon(_) => 3,
        }
    }

    /// Whether this shape gains and loses vertices at all. False for a point, which has
    /// exactly one and keeps it.
    pub fn is_vertex_list(&self) -> bool {
        !matches!(self, Self::Point(_))
    }

    /// The word for this shape in a message a GM reads.
    pub fn shape(&self) -> &'static str {
        match self {
            Self::Point(_) => "point",
            Self::Polyline(_) => "polyline",
            Self::Polygon(_) => "polygon",
        }
    }

    /// Whether this geometry holds at least the vertices its shape requires.
    pub fn has_enough_vertices(&self) -> bool {
        self.len() >= self.least()
    }

    /// Whether every vertex is finite.
    pub fn is_finite(&self) -> bool {
        self.vertices().iter().all(|vertex| vertex.is_finite())
    }

    pub(crate) fn vertices_mut(&mut self) -> Option<&mut Vec<CellPoint>> {
        match self {
            Self::Point(_) => None,
            Self::Polyline(vertices) | Self::Polygon(vertices) => Some(vertices),
        }
    }

    pub(crate) fn vertex_mut(&mut self, index: usize) -> Option<&mut CellPoint> {
        match self {
            Self::Point(vertex) if index == 0 => Some(vertex),
            Self::Point(_) => None,
            Self::Polyline(vertices) | Self::Polygon(vertices) => vertices.get_mut(index),
        }
    }
}

/// What a feature is, which decides how it is drawn.
///
/// It does not decide which note a feature gets: every feature is a place, whatever kind
/// it is, so [`NoteKind::of_a_feature`](crate::notebook::NoteKind::of_a_feature) takes no
/// argument.
///
/// Landcover and territory are separate kinds rather than one "region": the imported
/// terrain supplies only height and water, so every wood and every border is something
/// the GM drew, and a forest and a kingdom are not drawn the same way.
///
/// Deliberately not `#[non_exhaustive]`. A consumer matching on a kind to decide how to
/// draw it should fail to compile when a kind is added, rather than fall into a catch-all
/// arm that silently draws the new one as nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum FeatureKind {
    Settlement,
    DungeonEntry,
    Road,
    River,
    Trail,
    Landcover,
    Territory,
    Poi,
}

impl FeatureKind {
    /// Every kind, in the order a chooser offers them: the things a place is, then the
    /// lines between them, then the areas around them.
    ///
    /// Written out rather than derived, and the array length is the count — adding a
    /// variant without extending this fails to compile, which is the same guarantee the
    /// enum's lack of `#[non_exhaustive]` gives a `match`.
    pub const fn all() -> [Self; 8] {
        [
            Self::Settlement,
            Self::DungeonEntry,
            Self::Poi,
            Self::Road,
            Self::River,
            Self::Trail,
            Self::Landcover,
            Self::Territory,
        ]
    }
}

/// How big a settlement is, which decides the size of its icon and how early its label
/// wins a collision.
///
/// Only settlements carry one in practice, but nothing refuses it elsewhere: the styling
/// table answers for every kind-and-rank pair, so a rank on a road is a fact the table
/// simply does not consult rather than a document that fails to load.
///
/// Deliberately not derived from the polygon's area. A capital drawn small is still a
/// capital, and an area-derived rank would change what a settlement *is* every time the
/// GM adjusted its outline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Rank {
    Hamlet,
    Town,
    City,
}

impl Rank {
    /// Every rank, smallest first.
    pub const fn all() -> [Self; 3] {
        [Self::Hamlet, Self::Town, Self::City]
    }
}

/// One authored thing on a map: a geometry, what kind of thing it is, what it is called,
/// and what it hangs off.
///
/// The id is not a field — it is the key the [`World`](crate::world::World) holds the
/// feature under, so the two cannot come to disagree.
///
/// `parent` is what makes a city work: a settlement is a polygon, and the tavern inside
/// it is a point whose parent is that polygon. A world document never holds a parent
/// that names a missing feature, and never a cycle.
///
/// The fields are public because constructing one asserts nothing; every rule is checked
/// where a feature enters a world, not where it is built.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Feature {
    pub kind: FeatureKind,
    pub geometry: Geometry,
    pub label: String,
    /// Where this feature's note is, relative to the campaign's `notes` directory.
    /// Constrained by [`note_path_refusal`] wherever it is stored.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    /// The feature this one sits inside, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<FeatureId>,
    /// How big a settlement this is, if it is one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rank: Option<Rank>,
    /// The coarsest map scale this feature is drawn at, in terrain cells per logical
    /// pixel, or `None` to be drawn at every scale its parents allow.
    ///
    /// Logical rather than physical pixels so that a document authored on one display
    /// reveals identically on another. Constrained by [`reveal_refusal`] wherever it is
    /// stored: a value that is not a finite positive number would make its own comparison
    /// meaningless.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_cells_per_pixel: Option<f32>,
}

impl Feature {
    /// A feature of `kind` at `geometry`, with nothing else set.
    ///
    /// Every optional field is absent, which is what a freshly drawn shape carries: a
    /// label, a note, a parent, a rank and a reveal scale are all things the GM adds
    /// afterwards. Exists so that adding an optional field does not mean editing every
    /// struct literal in the workspace.
    pub fn plain(kind: FeatureKind, geometry: Geometry) -> Self {
        Self {
            kind,
            geometry,
            label: String::new(),
            note: None,
            parent: None,
            rank: None,
            max_cells_per_pixel: None,
        }
    }
}

/// Why `scale` is not one a feature may carry as its reveal threshold, or `None` if it is
/// fine.
///
/// A threshold is compared against the map's current cells-per-pixel every frame, so it
/// has to be a number that compares: `NaN` compares false against everything and would
/// hide the feature for ever, and zero or a negative value names a scale no camera
/// reaches.
pub fn reveal_refusal(scale: f32) -> Option<&'static str> {
    if !scale.is_finite() {
        return Some("is not a finite number");
    }
    if scale <= 0.0 {
        return Some("is not greater than zero, and names a scale no camera reaches");
    }
    None
}

/// Why `path` is not one a feature may carry as its note, or `None` if it is fine.
///
/// A note path is relative to the campaign's `notes` directory and is later handed to
/// `zk` as a command-line argument, so it may not be empty, may not be absolute, may not
/// carry a parent (`..`) component, and may not open with a dash.
///
/// The containment this gives is **lexical**: a path with no `..` component still leaves
/// the notes directory if something along it is a symlink. Every caller that opens the
/// result should treat it as a name inside a directory it trusts, not as proof of where
/// the bytes are.
pub fn note_path_refusal(path: &str) -> Option<&'static str> {
    if path.is_empty() {
        return Some("is empty");
    }
    if path.starts_with('-') {
        return Some("opens with a dash, which zk would read as an option");
    }

    let path = std::path::Path::new(path);
    if path.is_absolute() {
        return Some("is absolute, and a note path is relative to the notes directory");
    }
    for component in path.components() {
        match component {
            std::path::Component::Normal(_) | std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                return Some("climbs out of the notes directory with `..`");
            }
            std::path::Component::RootDir | std::path::Component::Prefix(_) => {
                return Some("is absolute, and a note path is relative to the notes directory");
            }
        }
    }
    None
}
