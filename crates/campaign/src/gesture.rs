//! The [`Edit`] one authoring gesture becomes.
//!
//! A gesture is not one change to the document — placing a tavern inside a city is an add
//! and a parent link, dragging a polyline is one move per vertex, deleting a settlement is
//! a delete per building — but it is one press, and so it must be one entry on the undo
//! stack. Every function here hands back a single [`Edit`] for that reason.
//!
//! The ordering inside a returned [`Edit::Batch`] is load-bearing rather than tidy:
//! [`Edit::Delete`] is refused while anything names the feature as parent, and a batch
//! refused part-way undoes its own prefix and returns nothing. A removal emitted in the
//! wrong order therefore does not half-apply — it silently does nothing at all.

use crate::edit::Edit;
use crate::feature::{CellPoint, Feature, FeatureId};
use crate::world::World;

/// What becomes of the features hanging off one that is being deleted.
///
/// There is no third answer that keeps the document coherent: a child whose parent is
/// gone is a dangling link, which [`World::load`](crate::world::World::load) refuses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Orphans {
    /// Delete the children too, and their children, all the way down.
    Cascade,
    /// Hand the children to the deleted feature's own parent, or to nothing when it had
    /// none.
    Promote,
}

/// The edit that adds `feature` under `id`, parented to the settlement it was placed
/// inside if there is one.
///
/// The parent is set on the feature before it is added rather than afterwards, so the
/// whole placement is one [`Edit::Add`] — a separate `SetParent` would be a second undo
/// step for something the GM did in one click.
///
/// `feature`'s existing parent is kept when it already has one; this only fills a gap.
pub fn place(world: &World, id: FeatureId, mut feature: Feature) -> Edit {
    if feature.parent.is_none() {
        let at = feature.geometry.vertices().first().copied();
        feature.parent = at.and_then(|at| crate::pick::enclosing_settlement(world, at));
    }
    Edit::Add { id, feature }
}

/// Where every vertex of `features` would be after moving by `offset`, in id order.
///
/// The one place a drag's offset is applied. An overlay drawing a drag in flight and the
/// [`Edit`] that lands it on release both go through here, so what is shown and what is
/// committed cannot come to disagree — which they would the moment the offset were
/// applied in two places.
///
/// Ids the world does not hold are skipped rather than refused: a selection can name a
/// feature an undo has since removed.
pub fn dragged(
    world: &World,
    features: &[FeatureId],
    offset: CellPoint,
) -> Vec<(FeatureId, Vec<CellPoint>)> {
    features
        .iter()
        .filter_map(|id| world.feature(*id).map(|feature| (*id, feature)))
        .map(|(id, feature)| {
            let moved = feature
                .geometry
                .vertices()
                .iter()
                .map(|vertex| CellPoint::new(vertex.x + offset.x, vertex.y + offset.y))
                .collect();
            (id, moved)
        })
        .collect()
}

/// The edit that moves every vertex of `features` by `offset`.
///
/// One batch however many features and vertices it covers, so a dragged 200-vertex
/// polyline undoes in one press rather than two hundred. Built from [`dragged`].
pub fn translate(world: &World, features: &[FeatureId], offset: CellPoint) -> Edit {
    let edits = dragged(world, features, offset)
        .into_iter()
        .flat_map(|(id, vertices)| {
            vertices
                .into_iter()
                .enumerate()
                .map(move |(index, to)| Edit::MoveVertex { id, index, to })
        })
        .collect();
    Edit::Batch(edits)
}

/// The edit that takes `id` out along with whatever hangs off it.
///
/// Both answers produce a batch that applies cleanly, and the order is why:
///
/// - [`Orphans::Cascade`] deletes the deepest descendants first and `id` last, so no
///   delete is ever attempted while something still names its target as parent.
/// - [`Orphans::Promote`] re-parents every direct child **before** the one delete, so by
///   the time `id` is removed nothing names it.
///
/// Emitted the other way round, either batch is refused on its first delete and rolls
/// itself back to nothing.
pub fn remove(world: &World, id: FeatureId, orphans: Orphans) -> Edit {
    match orphans {
        Orphans::Cascade => {
            let mut doomed = descendants(world, id);
            doomed.push(id);
            deepest_first(world, &mut doomed);
            Edit::Batch(doomed.into_iter().map(|id| Edit::Delete { id }).collect())
        }
        Orphans::Promote => {
            let grandparent = world.feature(id).and_then(|feature| feature.parent);
            let mut edits: Vec<Edit> = world
                .children_of(id)
                .map(|child| Edit::SetParent {
                    id: child,
                    parent: grandparent,
                })
                .collect();
            edits.push(Edit::Delete { id });
            Edit::Batch(edits)
        }
    }
}

/// The edit that takes every feature in `ids` out, and nothing else.
///
/// Deepest first, so a selection holding both a settlement and a building inside it does
/// not refuse itself. Only sound when nothing outside `ids` hangs off any of them — ask
/// [`outside_children`] first, because those are the ones the GM has to be asked about.
pub fn remove_all(world: &World, ids: &[FeatureId]) -> Edit {
    let mut doomed: Vec<FeatureId> = ids
        .iter()
        .copied()
        .filter(|id| world.feature(*id).is_some())
        .collect();
    deepest_first(world, &mut doomed);
    Edit::Batch(doomed.into_iter().map(|id| Edit::Delete { id }).collect())
}

/// Every feature in `ids` that something outside `ids` hangs off, with those children.
///
/// These are exactly the features a delete has to ask about: a child that is itself being
/// deleted needs no question, and a feature nothing hangs off needs none either. Empty
/// means the whole selection can be removed without asking.
pub fn outside_children(world: &World, ids: &[FeatureId]) -> Vec<(FeatureId, Vec<FeatureId>)> {
    ids.iter()
        .filter_map(|id| {
            let outside: Vec<FeatureId> = world
                .children_of(*id)
                .filter(|child| !ids.contains(child))
                .collect();
            (!outside.is_empty()).then_some((*id, outside))
        })
        .collect()
}

/// Every feature beneath `id`, at any depth, in no particular order.
pub fn descendants(world: &World, id: FeatureId) -> Vec<FeatureId> {
    let mut found = Vec::new();
    let mut frontier = vec![id];
    while let Some(next) = frontier.pop() {
        for child in world.children_of(next) {
            if !found.contains(&child) {
                found.push(child);
                frontier.push(child);
            }
        }
    }
    found
}

fn deepest_first(world: &World, ids: &mut [FeatureId]) {
    ids.sort_by(|a, b| depth(world, *b).cmp(&depth(world, *a)).then(b.cmp(a)));
}

fn depth(world: &World, id: FeatureId) -> usize {
    let mut depth = 0;
    let mut walked = 0;
    let mut current = world.feature(id).and_then(|feature| feature.parent);
    while let Some(parent) = current {
        depth += 1;
        walked += 1;
        if walked > world.len() {
            break;
        }
        current = world.feature(parent).and_then(|feature| feature.parent);
    }
    depth
}
