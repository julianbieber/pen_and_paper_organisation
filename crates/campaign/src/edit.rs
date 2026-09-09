//! The vocabulary of change: one value per way a world may differ from itself, and every
//! reason one is refused.
//!
//! An edit is a value rather than a method call, and that is the whole point. A button
//! builds one and a control socket parses one, and both then take the identical path
//! through [`Edit::apply`]; neither side can acquire a shortcut the other lacks, and
//! neither can change a world in a way the other could not have. Applying one hands back
//! the edit that undoes it, which is what undo and redo are built from.

use serde::{Deserialize, Serialize};

use crate::brush::TileChange;
use crate::feature::{
    CellPoint, Feature, FeatureId, FeatureKind, Rank, dungeon_name_refusal, note_path_refusal,
    reveal_refusal,
};
use crate::image::{ImageBackdrop, ImageProblem};
use crate::world::{ParentProblem, World};

/// Why an edit was refused.
///
/// Every variant names the feature it is about, because an edit a socket sent has no
/// other context to be reported in.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
#[non_exhaustive]
pub enum EditError {
    /// The world holds no feature under this id.
    #[error("there is no {0}")]
    NoSuchFeature(FeatureId),

    /// An add named an id the world already holds.
    #[error("{0} already exists")]
    IdInUse(FeatureId),

    /// A vertex index outside the geometry it indexes. An insert may name the vertex
    /// count itself, which appends; a move or a remove may not.
    #[error("{feature} has {len} vertices, so index {index} names none of them")]
    VertexOutOfBounds {
        feature: FeatureId,
        index: usize,
        len: usize,
    },

    /// An insert or a remove against a point, which has exactly one vertex and neither
    /// gains nor loses any.
    #[error("{feature} is a point: it has exactly one vertex and gains or loses none")]
    NotAVertexList { feature: FeatureId },

    /// A change that would leave a geometry with fewer vertices than its shape admits.
    #[error("{feature} would be a {shape} carrying {have} vertices, and a {shape} needs {least}")]
    TooFewVertices {
        feature: FeatureId,
        shape: &'static str,
        have: usize,
        least: usize,
    },

    /// A delete of a feature others hang off. The children are named so a caller can
    /// offer to do something with them without asking a second question.
    #[error("{feature} still holds {}, which must be dealt with first", children_list(.children))]
    HasChildren {
        feature: FeatureId,
        children: Vec<FeatureId>,
    },

    /// A parent link a world may not hold.
    #[error(transparent)]
    BadParent(#[from] ParentProblem),

    /// A coordinate that is not a finite number.
    #[error("{feature} cannot take {at}: a coordinate must be a finite number")]
    NonFiniteCoordinate { feature: FeatureId, at: CellPoint },

    /// A note path a feature may not carry.
    #[error("{feature} cannot take that note path: it {reason}")]
    BadNotePath {
        feature: FeatureId,
        reason: &'static str,
    },

    /// A reveal threshold a feature may not carry.
    #[error("{feature} cannot take a reveal scale of {scale}: it {reason}")]
    BadRevealScale {
        feature: FeatureId,
        scale: f32,
        reason: &'static str,
    },

    /// A dungeon name a feature may not carry.
    #[error("{feature} cannot take that dungeon name: it {reason}")]
    BadDungeonName {
        feature: FeatureId,
        reason: &'static str,
    },

    /// A dungeon name another feature in this world already carries.
    #[error("{feature} cannot take the dungeon `{name}`: {holder} already names it")]
    DungeonNameInUse {
        feature: FeatureId,
        holder: FeatureId,
        name: String,
    },

    /// A dungeon named on a feature inside a document that is itself a dungeon.
    #[error("{feature} is inside a dungeon, and a dungeon does not open another")]
    NestedDungeon { feature: FeatureId },

    /// A tile edit against a document whose backdrop is the terrain.
    #[error("this document is drawn on the terrain and has no grid to paint")]
    NoGrid,

    /// A tile edit naming a cell the grid does not hold.
    #[error("({x}, {y}) is outside a grid of {width}x{height}")]
    CellOutOfGrid {
        x: u32,
        y: u32,
        width: u32,
        height: u32,
    },

    /// An image edit against a document that declares no image backdrop.
    #[error("this document is drawn over no image, so there is nothing to place")]
    NoImage,

    /// An image declaration a document may not hold.
    #[error("that image backdrop cannot be drawn: {0}")]
    BadImage(#[from] ImageProblem),

    /// A declaration edit that would change nothing.
    ///
    /// Refused for the reason [`EditError::EmptyPaint`] is: clearing an image on a
    /// document that has none puts an entry on the undo stack that undoes nothing, and a
    /// press of undo that appears to do nothing is indistinguishable from one that was not
    /// noticed.
    #[error("this document is drawn over no image already")]
    NoImageChange,

    /// A tile edit that would change nothing.
    ///
    /// Refused rather than applied, so an entry on the undo stack always undoes
    /// something: a stroke that lands entirely on cells already carrying the tile is a
    /// gesture that did not author anything, and a press of undo that appears to do
    /// nothing is indistinguishable from one that was not noticed.
    #[error("that stroke changes no cell")]
    EmptyPaint,
}

fn children_list(children: &[FeatureId]) -> String {
    children
        .iter()
        .map(FeatureId::to_string)
        .collect::<Vec<String>>()
        .join(", ")
}

/// A change to a world document, as a value.
///
/// Deliberately not `#[non_exhaustive]`: a consumer matching on an edit — a socket
/// describing one, an editor deciding what to redraw — should fail to compile when a
/// variant is added, rather than fall into a catch-all arm that silently ignores it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Edit {
    /// Put `feature` into the world under `id`. The caller chooses the id, from
    /// [`World::fresh_id`], so that the inverse of a delete restores a feature under the
    /// id it had.
    Add { id: FeatureId, feature: Feature },
    /// Take a feature out. Refused while anything names it as parent.
    Delete { id: FeatureId },
    /// Move one vertex to another position.
    MoveVertex {
        id: FeatureId,
        index: usize,
        to: CellPoint,
    },
    /// Put a new vertex at `index`, which may be the vertex count, meaning append.
    InsertVertex {
        id: FeatureId,
        index: usize,
        at: CellPoint,
    },
    /// Take the vertex at `index` out.
    RemoveVertex { id: FeatureId, index: usize },
    /// Change what kind of thing a feature is.
    SetKind { id: FeatureId, kind: FeatureKind },
    /// Change what a feature is called.
    SetLabel { id: FeatureId, label: String },
    /// Point a feature at a note, or at none.
    SetNote {
        id: FeatureId,
        note: Option<String>,
    },
    /// Put a feature inside another, or outside everything.
    SetParent {
        id: FeatureId,
        parent: Option<FeatureId>,
    },
    /// Say how big a settlement is, or that it has no rank.
    SetRank { id: FeatureId, rank: Option<Rank> },
    /// Say the coarsest scale a feature is drawn at, in cells per logical pixel, or that
    /// it is drawn at every scale its parents allow.
    SetMaxCellsPerPixel {
        id: FeatureId,
        scale: Option<f32>,
    },
    /// Point a feature at the dungeon it opens, or at none.
    SetDungeon {
        id: FeatureId,
        dungeon: Option<String>,
    },
    /// Put tiles into the document's grid.
    ///
    /// One edit however many cells the stroke covered, which is what makes a brush
    /// stroke a single press of undo without a [`Edit::Batch`] holding a change per cell.
    PaintTiles { changes: Vec<TileChange> },
    /// Declare the picture this document is drawn over, or clear it.
    ///
    /// What an import lands. Refused when it would clear an image the document does not
    /// have, so the entry it puts on the undo stack always undoes something.
    SetImage { image: Option<ImageBackdrop> },
    /// Move and scale the picture together.
    ///
    /// One edit for a whole drag, a whole corner scale or a whole calibration, which is
    /// what makes each of them a single press of undo. Origin and scale travel together
    /// because every one of those gestures moves both: a rescale is anchored on a point
    /// that stays put, and holding the anchor fixed *is* moving the origin.
    PlaceImage {
        origin: CellPoint,
        cells_per_pixel: f32,
    },
    /// Fade the picture, or make it solid.
    SetImageOpacity { opacity: f32 },
    /// Several edits that apply, undo and redo as one.
    ///
    /// What makes deleting a settlement together with everything parented to it a single
    /// press of undo rather than one per building.
    Batch(Vec<Edit>),
}

impl Edit {
    /// Apply this edit to `world` and hand back the edit that undoes it.
    ///
    /// Applying an edit and then the edit it returned leaves the world exactly as it was,
    /// `next_id` included: no edit reads or writes that counter, because allocating an id
    /// is not a change to the feature set.
    ///
    /// A refusal leaves the world exactly as it was and returns no inverse. That holds
    /// for [`Edit::Batch`] too — a batch refused part-way undoes the prefix it had
    /// already applied before returning, so a batch is as atomic as any other edit.
    ///
    /// Every rule [`World::load`](crate::world::World::load) enforces is enforced here,
    /// so an edit cannot build a world that would be refused if it were saved and opened
    /// again.
    ///
    /// # Panics
    ///
    /// Never, for any edit built against a world in hand. A batch rollback applies
    /// inverses it has just produced, newest first, and one of those failing would mean
    /// an inverse did not describe the change it came from.
    pub fn apply(self, world: &mut World) -> Result<Self, EditError> {
        match self {
            Self::Add { id, feature } => {
                if world.feature(id).is_some() {
                    return Err(EditError::IdInUse(id));
                }
                check_geometry(id, &feature)?;
                check_note(id, feature.note.as_deref())?;
                check_reveal(id, feature.max_cells_per_pixel)?;
                check_dungeon(world, id, feature.dungeon.as_deref())?;
                world.may_take_parent(id, feature.parent)?;

                world.features_mut().insert(id, feature);
                Ok(Self::Delete { id })
            }

            Self::Delete { id } => {
                if world.feature(id).is_none() {
                    return Err(EditError::NoSuchFeature(id));
                }
                let children: Vec<FeatureId> = world.children_of(id).collect();
                if !children.is_empty() {
                    return Err(EditError::HasChildren {
                        feature: id,
                        children,
                    });
                }

                let feature = world
                    .features_mut()
                    .remove(&id)
                    .expect("the feature was there a moment ago");
                Ok(Self::Add { id, feature })
            }

            Self::MoveVertex { id, index, to } => {
                let geometry = &feature_of(world, id)?.geometry;
                bounds(id, geometry.len(), index, false)?;
                if !to.is_finite() {
                    return Err(EditError::NonFiniteCoordinate { feature: id, at: to });
                }

                let vertex = world
                    .features_mut()
                    .get_mut(&id)
                    .and_then(|feature| feature.geometry.vertex_mut(index))
                    .expect("the vertex was in bounds a moment ago");
                let was = std::mem::replace(vertex, to);
                Ok(Self::MoveVertex { id, index, to: was })
            }

            Self::InsertVertex { id, index, at } => {
                let geometry = &feature_of(world, id)?.geometry;
                if !geometry.is_vertex_list() {
                    return Err(EditError::NotAVertexList { feature: id });
                }
                bounds(id, geometry.len(), index, true)?;
                if !at.is_finite() {
                    return Err(EditError::NonFiniteCoordinate { feature: id, at });
                }

                world
                    .features_mut()
                    .get_mut(&id)
                    .and_then(|feature| feature.geometry.vertices_mut())
                    .expect("a vertex list a moment ago")
                    .insert(index, at);
                Ok(Self::RemoveVertex { id, index })
            }

            Self::RemoveVertex { id, index } => {
                let geometry = &feature_of(world, id)?.geometry;
                if !geometry.is_vertex_list() {
                    return Err(EditError::NotAVertexList { feature: id });
                }
                bounds(id, geometry.len(), index, false)?;
                if geometry.len() == geometry.least() {
                    return Err(EditError::TooFewVertices {
                        feature: id,
                        shape: geometry.shape(),
                        have: geometry.len() - 1,
                        least: geometry.least(),
                    });
                }

                let was = world
                    .features_mut()
                    .get_mut(&id)
                    .and_then(|feature| feature.geometry.vertices_mut())
                    .expect("a vertex list a moment ago")
                    .remove(index);
                Ok(Self::InsertVertex { id, index, at: was })
            }

            Self::SetKind { id, kind } => {
                feature_of(world, id)?;
                let was = std::mem::replace(&mut feature_mut(world, id).kind, kind);
                Ok(Self::SetKind { id, kind: was })
            }

            Self::SetLabel { id, label } => {
                feature_of(world, id)?;
                let was = std::mem::replace(&mut feature_mut(world, id).label, label);
                Ok(Self::SetLabel { id, label: was })
            }

            Self::SetNote { id, note } => {
                feature_of(world, id)?;
                check_note(id, note.as_deref())?;
                let was = std::mem::replace(&mut feature_mut(world, id).note, note);
                Ok(Self::SetNote { id, note: was })
            }

            Self::SetParent { id, parent } => {
                feature_of(world, id)?;
                world.may_take_parent(id, parent)?;
                let was = std::mem::replace(&mut feature_mut(world, id).parent, parent);
                Ok(Self::SetParent { id, parent: was })
            }

            Self::SetRank { id, rank } => {
                feature_of(world, id)?;
                let was = std::mem::replace(&mut feature_mut(world, id).rank, rank);
                Ok(Self::SetRank { id, rank: was })
            }

            Self::SetMaxCellsPerPixel { id, scale } => {
                feature_of(world, id)?;
                check_reveal(id, scale)?;
                let was = std::mem::replace(&mut feature_mut(world, id).max_cells_per_pixel, scale);
                Ok(Self::SetMaxCellsPerPixel { id, scale: was })
            }

            Self::SetDungeon { id, dungeon } => {
                feature_of(world, id)?;
                check_dungeon(world, id, dungeon.as_deref())?;
                let was = std::mem::replace(&mut feature_mut(world, id).dungeon, dungeon);
                Ok(Self::SetDungeon { id, dungeon: was })
            }

            Self::SetImage { image } => {
                if image.is_none() && world.image().is_none() {
                    return Err(EditError::NoImageChange);
                }
                if let Some(image) = &image
                    && let Some(problem) = image.refusal()
                {
                    return Err(EditError::BadImage(problem));
                }
                let was = std::mem::replace(world.image_mut(), image);
                Ok(Self::SetImage { image: was })
            }

            Self::PlaceImage {
                origin,
                cells_per_pixel,
            } => {
                let image = world.image().ok_or(EditError::NoImage)?;
                let moved = image.placed(origin, cells_per_pixel)?;
                let was = world
                    .image_mut()
                    .replace(moved)
                    .expect("the image was there a moment ago");
                Ok(Self::PlaceImage {
                    origin: was.origin(),
                    cells_per_pixel: was.cells_per_pixel(),
                })
            }

            Self::SetImageOpacity { opacity } => {
                let image = world.image().ok_or(EditError::NoImage)?;
                let faded = image.faded(opacity)?;
                let was = world
                    .image_mut()
                    .replace(faded)
                    .expect("the image was there a moment ago");
                Ok(Self::SetImageOpacity {
                    opacity: was.opacity(),
                })
            }

            Self::PaintTiles { changes } => {
                let grid = world.grid().ok_or(EditError::NoGrid)?;

                for change in &changes {
                    if !grid.holds(i64::from(change.x), i64::from(change.y)) {
                        return Err(EditError::CellOutOfGrid {
                            x: change.x,
                            y: change.y,
                            width: grid.width(),
                            height: grid.height(),
                        });
                    }
                }

                let mut wanted: Vec<TileChange> = Vec::with_capacity(changes.len());
                for change in changes {
                    if let Some(seen) = wanted
                        .iter_mut()
                        .find(|held| held.x == change.x && held.y == change.y)
                    {
                        seen.tile = change.tile;
                    } else {
                        wanted.push(change);
                    }
                }
                wanted.retain(|change| {
                    grid.get(i64::from(change.x), i64::from(change.y)) != Some(change.tile)
                });
                if wanted.is_empty() {
                    return Err(EditError::EmptyPaint);
                }

                let grid = world.grid_mut().expect("the grid was there a moment ago");
                let was: Vec<TileChange> = wanted
                    .iter()
                    .map(|change| {
                        let tile = grid
                            .set(i64::from(change.x), i64::from(change.y), change.tile)
                            .expect("the cell was in bounds a moment ago");
                        TileChange {
                            x: change.x,
                            y: change.y,
                            tile,
                        }
                    })
                    .collect();
                Ok(Self::PaintTiles { changes: was })
            }

            Self::Batch(edits) => {
                let mut inverses: Vec<Self> = Vec::with_capacity(edits.len());
                for edit in edits {
                    match edit.apply(world) {
                        Ok(inverse) => inverses.push(inverse),
                        Err(refusal) => {
                            for inverse in inverses.into_iter().rev() {
                                inverse
                                    .apply(world)
                                    .expect("an inverse must undo the edit it came from");
                            }
                            return Err(refusal);
                        }
                    }
                }
                inverses.reverse();
                Ok(Self::Batch(inverses))
            }
        }
    }
}

fn feature_of(world: &World, id: FeatureId) -> Result<&Feature, EditError> {
    world.feature(id).ok_or(EditError::NoSuchFeature(id))
}

fn feature_mut(world: &mut World, id: FeatureId) -> &mut Feature {
    world
        .features_mut()
        .get_mut(&id)
        .expect("the feature was there a moment ago")
}

fn bounds(id: FeatureId, len: usize, index: usize, appending: bool) -> Result<(), EditError> {
    let past_end = if appending { index > len } else { index >= len };
    if past_end {
        return Err(EditError::VertexOutOfBounds {
            feature: id,
            index,
            len,
        });
    }
    Ok(())
}

fn check_geometry(id: FeatureId, feature: &Feature) -> Result<(), EditError> {
    if let Some(vertex) = feature
        .geometry
        .vertices()
        .iter()
        .find(|vertex| !vertex.is_finite())
    {
        return Err(EditError::NonFiniteCoordinate {
            feature: id,
            at: *vertex,
        });
    }
    if !feature.geometry.has_enough_vertices() {
        return Err(EditError::TooFewVertices {
            feature: id,
            shape: feature.geometry.shape(),
            have: feature.geometry.len(),
            least: feature.geometry.least(),
        });
    }
    Ok(())
}

fn check_reveal(id: FeatureId, scale: Option<f32>) -> Result<(), EditError> {
    if let Some(scale) = scale
        && let Some(reason) = reveal_refusal(scale)
    {
        return Err(EditError::BadRevealScale {
            feature: id,
            scale,
            reason,
        });
    }
    Ok(())
}

fn check_dungeon(world: &World, id: FeatureId, dungeon: Option<&str>) -> Result<(), EditError> {
    let Some(name) = dungeon else {
        return Ok(());
    };
    if let Some(reason) = dungeon_name_refusal(name) {
        return Err(EditError::BadDungeonName { feature: id, reason });
    }
    if world.grid().is_some() {
        return Err(EditError::NestedDungeon { feature: id });
    }
    if let Some((holder, _)) = world
        .features()
        .find(|(other, feature)| *other != id && feature.dungeon.as_deref() == Some(name))
    {
        return Err(EditError::DungeonNameInUse {
            feature: id,
            holder,
            name: name.to_owned(),
        });
    }
    Ok(())
}

fn check_note(id: FeatureId, note: Option<&str>) -> Result<(), EditError> {
    if let Some(note) = note
        && let Some(reason) = note_path_refusal(note)
    {
        return Err(EditError::BadNotePath {
            feature: id,
            reason,
        });
    }
    Ok(())
}
