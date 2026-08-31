//! A shape part-way through being drawn, and the feature it becomes when it is finished.
//!
//! A draft is deliberately not a [`Geometry`]: a polyline carrying one vertex is a
//! perfectly good draft and an illegal geometry, so a half-drawn shape has nowhere else
//! it could live. That is the whole reason this type exists — the [`World`](crate::world::World)
//! never sees a shape that would not load.

use crate::feature::{CellPoint, Feature, FeatureKind, Geometry};

/// Which shape a draft is heading towards.
///
/// Separate from [`Geometry`] because it carries no vertices, and a draft's vertices are
/// still being collected. [`DraftShape::least`] asks the geometry it will become, so the
/// two minimums cannot drift apart.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DraftShape {
    Point,
    Polyline,
    Polygon,
}

impl DraftShape {
    /// The fewest vertices this shape admits, asked of the geometry it becomes.
    pub fn least(self) -> usize {
        self.empty_geometry().least()
    }

    /// The word for this shape in a message a GM reads.
    pub fn shape(self) -> &'static str {
        self.empty_geometry().shape()
    }

    fn empty_geometry(self) -> Geometry {
        match self {
            Self::Point => Geometry::Point(CellPoint::new(0.0, 0.0)),
            Self::Polyline => Geometry::Polyline(Vec::new()),
            Self::Polygon => Geometry::Polygon(Vec::new()),
        }
    }
}

/// A shape being drawn: what it will be, and the vertices placed so far.
#[derive(Debug, Clone, PartialEq)]
pub struct Draft {
    pub kind: FeatureKind,
    pub shape: DraftShape,
    vertices: Vec<CellPoint>,
}

impl Draft {
    /// A draft of `shape`, to become a feature of `kind`, with nothing placed yet.
    pub fn new(kind: FeatureKind, shape: DraftShape) -> Self {
        Self {
            kind,
            shape,
            vertices: Vec::new(),
        }
    }

    /// The vertices placed so far, in the order they were placed.
    pub fn vertices(&self) -> &[CellPoint] {
        &self.vertices
    }

    /// How many vertices have been placed.
    pub fn len(&self) -> usize {
        self.vertices.len()
    }

    /// Whether nothing has been placed yet.
    pub fn is_empty(&self) -> bool {
        self.vertices.is_empty()
    }

    /// Place a vertex at the end. A non-finite position is ignored, so a draft never
    /// carries a coordinate the document would refuse.
    pub fn push(&mut self, at: CellPoint) {
        if at.is_finite() {
            self.vertices.push(at);
        }
    }

    /// Take the last vertex back off, if there is one — what Backspace does while drawing.
    pub fn pop(&mut self) -> Option<CellPoint> {
        self.vertices.pop()
    }

    /// Whether [`Draft::finish`] would yield a feature rather than nothing.
    pub fn can_finish(&self) -> bool {
        self.vertices.len() >= self.shape.least()
    }

    /// The feature this draft describes, or `None` when it holds fewer vertices than its
    /// shape admits.
    ///
    /// A polygon is closed by the geometry itself — the edge from its last vertex back to
    /// its first is not stored — so finishing one adds no vertex.
    ///
    /// Consumes the draft: a finished shape is a feature, and there is nothing left to
    /// keep drawing.
    pub fn finish(self) -> Option<Feature> {
        if !self.can_finish() {
            return None;
        }
        let geometry = match self.shape {
            DraftShape::Point => Geometry::Point(*self.vertices.first()?),
            DraftShape::Polyline => Geometry::Polyline(self.vertices),
            DraftShape::Polygon => Geometry::Polygon(self.vertices),
        };
        Some(Feature {
            kind: self.kind,
            geometry,
            label: String::new(),
            note: None,
            parent: None,
        })
    }
}
