//! The vocabulary of change: one value per way a world may differ from itself, and every
//! reason one is refused.
//!
//! An edit is a value rather than a method call, and that is the whole point. A button
//! builds one and a control socket parses one, and both then take the identical path
//! through [`Edit::apply`]; neither side can acquire a shortcut the other lacks, and
//! neither can change a world in a way the other could not have. Applying one hands back
//! the edit that undoes it, which is what undo and redo are built from.

use serde::{Deserialize, Serialize};

use crate::feature::{CellPoint, Feature, FeatureId, FeatureKind, note_path_refusal};
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
