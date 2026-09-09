//! What is selected, and carrying a drag until it lands as one [`Edit`].
//!
//! A drag in flight has not touched the document at all. It is followed in the resource
//! and drawn from there, and only the release builds an edit — a document changed sixty
//! times a second would fill the undo stack with a single gesture, and undoing a drag
//! would then take as many presses as it took frames.
//!
//! Because the gesture outlives the press, the "is the pointer over the UI" gate applies
//! to the press alone. Applied to the whole system it would abandon a drag the moment it
//! crossed the tool strip, and the release would never be seen at all.
//!
//! Every hit test here is filtered by the same detail the draw pass used, so a press
//! cannot land on a feature that frame declined to draw. The filter is built once per
//! gesture from [`pickable`] and handed to `campaign::pick`, which does not otherwise know
//! what is on screen.

use bevy::prelude::*;
use campaign::edit::Edit;
use campaign::feature::{CellPoint, FeatureId};
use campaign::pick::{self, Hit};
use campaign::{gesture, world::World};

use crate::StatusMessage;
use crate::document::{self as doc, WorldDoc};
use crate::map::pointer::{MapPointer, PICK_SLACK_PIXELS, SNAP_SLACK_PIXELS};

/// Which vertex is selected, and which feature it belongs to.
///
/// The feature is part of it because a bare index says nothing when several features are
/// selected, and because an index alone survives an edit that shifted it — silently
/// naming a different vertex than the one the GM picked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SelectedVertex {
    pub feature: FeatureId,
    pub index: usize,
}

/// What a held drag is doing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DragKind {
    Vertex,
    Body,
    Box,
}

/// What is selected on the map.
#[derive(Resource, Debug, Default)]
pub struct Selection {
    pub features: Vec<FeatureId>,
    pub vertex: Option<SelectedVertex>,
}

impl Selection {
    /// Select `id` alone.
    pub fn replace_with(&mut self, id: FeatureId) {
        self.features.clear();
        self.features.push(id);
        self.vertex = None;
    }

    /// Add `id` to the selection, or take it out if it was already in.
    pub fn toggle(&mut self, id: FeatureId) {
        if let Some(at) = self.features.iter().position(|held| *held == id) {
            self.features.remove(at);
        } else {
            self.features.push(id);
        }
        self.vertex = None;
    }

    /// Select nothing.
    pub fn clear(&mut self) {
        self.features.clear();
        self.vertex = None;
    }

    /// Whether anything is selected.
    pub fn is_empty(&self) -> bool {
        self.features.is_empty()
    }

    /// The one selected feature, or `None` when none or several are.
    pub fn only(&self) -> Option<FeatureId> {
        match self.features.as_slice() {
            [id] => Some(*id),
            _ => None,
        }
    }

    /// Drop whatever the document no longer holds.
    ///
    /// An undo can remove a selected feature and a cascade several; an insert or a remove
    /// shifts every later vertex index. A stale id is caught by the edit that uses it, but
    /// a stale *index* is not — it silently edits a different vertex — so the vertex goes
    /// the moment its geometry could have moved under it.
    pub fn reconcile(&mut self, world: &World) {
        self.features.retain(|id| world.feature(*id).is_some());
        let held = self.vertex.filter(|vertex| {
            world
                .feature(vertex.feature)
                .is_some_and(|feature| vertex.index < feature.geometry.len())
        });
        self.vertex = held;
    }
}

/// A drag being carried, in terrain cells.
///
/// Cells rather than screen pixels so that a pan or a zoom mid-drag moves the map under
/// the gesture without moving what is being dragged.
#[derive(Resource, Debug)]
pub struct Dragging {
    pub what: Option<DragKind>,
    pub grabbed: CellPoint,
    pub latest: CellPoint,
}

impl Default for Dragging {
    fn default() -> Self {
        Self {
            what: None,
            grabbed: CellPoint::new(0.0, 0.0),
            latest: CellPoint::new(0.0, 0.0),
        }
    }
}

impl Dragging {
    /// How far the drag has come, in cells.
    pub fn offset(&self) -> CellPoint {
        CellPoint::new(self.latest.x - self.grabbed.x, self.latest.y - self.grabbed.y)
    }

    /// Whether the drag has actually moved anything.
    pub fn moved(&self) -> bool {
        let offset = self.offset();
        offset.x != 0.0 || offset.y != 0.0
    }

    /// Forget the drag without landing it.
    pub fn cancel(&mut self) {
        self.what = None;
    }
}

/// Clicks to select, and carries a drag until it lands as one edit.
///
/// A drag that moved nothing lands nothing: a bare selecting click leaves the undo stack
/// and the dirty flag alone, so closing after only looking at a feature does not prompt.
/// A press on an edge of an already-selected feature inserts a vertex there immediately
/// rather than on release, because dragging that new vertex is the next gesture.
///
/// A press lands only on a feature the map is currently drawing, judged by the same
/// [`campaign::lod`] answer the draw pass used — so a click on apparently empty ground
/// selects nothing and a box drag catches nothing that was not on screen. The current
/// selection is exempt from that, or a feature could not be clicked again to deselect it.
pub fn select_features(
    buttons: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    pointer: Res<MapPointer>,
    over_ui: Res<super::PointerOverUi>,
    mut selection: ResMut<Selection>,
    mut dragging: ResMut<Dragging>,
    mut doc: ResMut<WorldDoc>,
    mut status: ResMut<StatusMessage>,
) {
    let Some(cell) = pointer.cell else {
        return;
    };

    if buttons.just_pressed(MouseButton::Left) && !over_ui.0 {
        begin(&mut selection, &mut dragging, &mut doc, &mut status, &keys, cell, &pointer);
        return;
    }

    if dragging.what.is_some() {
        dragging.latest = cell;
    }

    if buttons.just_released(MouseButton::Left) && dragging.what.is_some() {
        land(&mut selection, &mut dragging, &mut doc, &mut status, &pointer);
    }
}

fn pickable<'a>(
    doc: &'a WorldDoc,
    selection: &'a Selection,
    pointer: &MapPointer,
) -> impl Fn(FeatureId) -> bool + 'a {
    let cells_per_pixel = pointer.cells_per_pixel;
    move |id| {
        campaign::lod::is_pickable(
            doc.document.world(),
            &doc.areas,
            cells_per_pixel,
            id,
            &selection.features,
        )
    }
}

fn begin(
    selection: &mut Selection,
    dragging: &mut Dragging,
    doc: &mut WorldDoc,
    status: &mut StatusMessage,
    keys: &ButtonInput<KeyCode>,
    cell: CellPoint,
    pointer: &MapPointer,
) {
    dragging.grabbed = cell;
    dragging.latest = cell;

    let landing = {
        let drawn = pickable(doc, selection, pointer);
        pick::pick(doc.document.world(), cell, pointer.slack(PICK_SLACK_PIXELS), &drawn)
    };
    let adding = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);

    let Some(landing) = landing else {
        selection.clear();
        dragging.what = Some(DragKind::Box);
        return;
    };

    let splitting =
        landing.hit == Hit::Edge && selection.features.contains(&landing.feature);

    match landing.hit {
        Hit::Vertex => {
            selection.replace_with(landing.feature);
            selection.vertex = Some(SelectedVertex {
                feature: landing.feature,
                index: landing.index,
            });
            dragging.what = Some(DragKind::Vertex);
        }
        _ if splitting => {
            let applied = doc::apply(
                doc,
                status,
                Edit::InsertVertex {
                    id: landing.feature,
                    index: landing.index,
                    at: cell,
                },
            );
            dragging.what = None;
            if applied {
                selection.vertex = Some(SelectedVertex {
                    feature: landing.feature,
                    index: landing.index,
                });
            }
        }
        _ => {
            if adding {
                selection.toggle(landing.feature);
            } else if !selection.features.contains(&landing.feature) {
                selection.replace_with(landing.feature);
            }
            dragging.what = Some(DragKind::Body);
        }
    }
}

fn land(
    selection: &mut Selection,
    dragging: &mut Dragging,
    doc: &mut WorldDoc,
    status: &mut StatusMessage,
    pointer: &MapPointer,
) {
    let what = dragging.what.take();
    let offset = dragging.offset();

    match what {
        Some(DragKind::Box) => {
            let caught = {
                let drawn = pickable(doc, selection, pointer);
                pick::within(doc.document.world(), dragging.grabbed, dragging.latest, &drawn)
            };
            selection.features = caught;
            selection.vertex = None;
        }
        _ if !dragging.moved() => {}
        Some(DragKind::Vertex) => {
            let Some(vertex) = selection.vertex else {
                return;
            };
            let world = doc.document.world();
            let Some(from) = world
                .feature(vertex.feature)
                .and_then(|feature| feature.geometry.vertices().get(vertex.index).copied())
            else {
                return;
            };
            let moved = CellPoint::new(from.x + offset.x, from.y + offset.y);
            let snapped = {
                let drawn = pickable(doc, selection, pointer);
                let world = doc.document.world();
                pick::snap(world, &[], moved, pointer.slack(SNAP_SLACK_PIXELS), &drawn)
            };
            doc::apply(
                doc,
                status,
                Edit::MoveVertex {
                    id: vertex.feature,
                    index: vertex.index,
                    to: snapped.at,
                },
            );
        }
        Some(DragKind::Body) => {
            let edit = gesture::translate(doc.document.world(), &selection.features, offset);
            doc::apply(doc, status, edit);
        }
        None => {}
    }
}

/// Drops whatever the document no longer holds from the selection.
///
/// Its own system so the rule is in one place: undo, redo, delete and vertex insertion
/// can each invalidate a selection, and each of them remembering to reconcile is four
/// chances to forget.
pub fn reconcile_selection(doc: Res<WorldDoc>, mut selection: ResMut<Selection>) {
    if !doc.is_changed() {
        return;
    }
    selection.reconcile(doc.document.world());
}

#[cfg(test)]
mod tests {
    use super::*;
    use campaign::feature::{Feature, FeatureKind, Geometry};
    use campaign::{Document, World};

    use crate::document::WorldDoc;
    use crate::features::PointerOverUi;

    fn at(x: f32, y: f32) -> CellPoint {
        CellPoint::new(x, y)
    }

    fn road() -> (World, FeatureId) {
        let mut world = World::default();
        let id = world.fresh_id();
        Edit::Add {
            id,
            feature: Feature::plain(
                FeatureKind::Road,
                Geometry::Polyline(vec![at(0.0, 0.0), at(100.0, 0.0)]),
            ),
        }
        .apply(&mut world)
        .expect("the fixture must be addable");
        (world, id)
    }

    fn app_with(world: World) -> App {
        let mut app = App::new();
        app.init_resource::<ButtonInput<MouseButton>>()
            .init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<Selection>()
            .init_resource::<Dragging>()
            .init_resource::<PointerOverUi>()
            .init_resource::<StatusMessage>()
            .insert_resource(MapPointer {
                cell: Some(at(0.0, 0.0)),
                cells_per_pixel: 1.0,
            })
            .insert_resource(WorldDoc::world_map(Document::new(world), std::path::PathBuf::from("world.ron")))
            .add_systems(Update, select_features);
        app
    }

    fn point_at(app: &mut App, cell: CellPoint) {
        app.world_mut().resource_mut::<MapPointer>().cell = Some(cell);
    }

    fn press(app: &mut App) {
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .press(MouseButton::Left);
    }

    fn release(app: &mut App) {
        let mut buttons = app.world_mut().resource_mut::<ButtonInput<MouseButton>>();
        buttons.clear();
        buttons.release(MouseButton::Left);
    }

    fn step(app: &mut App) {
        app.update();
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .clear();
    }

    fn undo_depth(app: &App) -> usize {
        app.world().resource::<WorldDoc>().document.undo_depth()
    }

    // A bare click to select must cost nothing: an entry on the undo stack here means that
    // closing after only looking at a feature prompts about unsaved changes.
    #[test]
    fn a_click_that_moves_nothing_lands_no_edit() {
        let (world, id) = road();
        let mut app = app_with(world);

        point_at(&mut app, at(50.0, 0.0));
        press(&mut app);
        step(&mut app);
        release(&mut app);
        step(&mut app);

        assert_eq!(app.world().resource::<Selection>().features, vec![id]);
        assert_eq!(undo_depth(&app), 0, "a selecting click is not a change");
        assert!(!app.world().resource::<WorldDoc>().document.is_dirty());
    }

    // A drag of any length is one entry, and the document does not see it until release —
    // otherwise one gesture fills the undo stack and undoing it takes as many presses as
    // it took frames.
    #[test]
    fn a_body_drag_lands_exactly_one_edit_and_only_on_release() {
        let (world, id) = road();
        let mut app = app_with(world);

        point_at(&mut app, at(50.0, 0.0));
        press(&mut app);
        step(&mut app);

        for x in [55.0, 60.0, 65.0] {
            point_at(&mut app, at(x, 10.0));
            step(&mut app);
            assert_eq!(undo_depth(&app), 0, "a held drag has not touched the document");
        }

        release(&mut app);
        step(&mut app);

        assert_eq!(undo_depth(&app), 1, "a whole drag is one entry");
        let doc = app.world().resource::<WorldDoc>();
        let moved = doc.document.world().feature(id).unwrap().geometry.vertices();
        assert_eq!(moved[0], at(15.0, 10.0));
        assert_eq!(moved[1], at(115.0, 10.0));
    }

    // The UI-hover gate applies to the press alone. Applied to the whole system, a drag
    // released over the tool strip would never land and would stay held for ever.
    #[test]
    fn a_drag_released_over_the_ui_still_lands() {
        let (world, _) = road();
        let mut app = app_with(world);

        point_at(&mut app, at(50.0, 0.0));
        press(&mut app);
        step(&mut app);

        point_at(&mut app, at(60.0, 0.0));
        app.world_mut().resource_mut::<PointerOverUi>().0 = true;
        release(&mut app);
        step(&mut app);

        assert_eq!(undo_depth(&app), 1);
        assert!(
            app.world().resource::<Dragging>().what.is_none(),
            "the drag must not be left held"
        );
    }

    // The other half of the same rule: a press that begins over the UI is not a press on
    // the map, so a click on the toolbar never selects or clears anything.
    #[test]
    fn a_press_that_begins_over_the_ui_does_not_author() {
        let (world, _) = road();
        let mut app = app_with(world);

        app.world_mut().resource_mut::<PointerOverUi>().0 = true;
        point_at(&mut app, at(50.0, 0.0));
        press(&mut app);
        step(&mut app);

        assert!(app.world().resource::<Selection>().is_empty());
        assert!(app.world().resource::<Dragging>().what.is_none());
    }

    // An insert shifts every later index, and a stale index silently edits a different
    // vertex than the one that was picked — which no EditError catches.
    #[test]
    fn a_selected_vertex_is_dropped_once_its_index_could_have_moved() {
        let (mut world, id) = road();
        let mut selection = Selection::default();
        selection.replace_with(id);
        selection.vertex = Some(SelectedVertex {
            feature: id,
            index: 1,
        });

        Edit::Delete { id }.apply(&mut world).expect("the road is deletable");
        selection.reconcile(&world);

        assert!(selection.features.is_empty());
        assert!(selection.vertex.is_none());
    }
}
