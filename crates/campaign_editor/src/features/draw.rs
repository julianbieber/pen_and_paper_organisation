//! Turning clicks into a draft, and a finished draft into an [`Edit`].
//!
//! The draft is the only place a half-drawn shape exists. Nothing partial ever reaches the
//! document, so every state the GM can see mid-stroke is one the world document would
//! never have to represent.

use bevy::prelude::*;
use campaign::draft::{Draft, DraftShape};
use campaign::pick::Snap;
use campaign::{gesture, pick};

use crate::StatusMessage;
use crate::features::doc::{self, WorldDoc};
use crate::features::select::Selection;
use crate::features::tool::ActiveTool;
use crate::map::pointer::{MapPointer, PICK_SLACK_PIXELS, SNAP_SLACK_PIXELS};

/// The shape being drawn, and where its next vertex would land.
///
/// `snap` is worked out once per frame here and read by the overlay, rather than each of
/// them scanning the world for a target: two answers to "what would this vertex snap to"
/// is one more than the GM can be shown.
#[derive(Resource, Debug, Default)]
pub struct Drafting {
    pub draft: Option<Draft>,
    pub snap: Option<Snap>,
}

impl Drafting {
    /// Throw away whatever is being drawn.
    pub fn abandon(&mut self) {
        self.draft = None;
        self.snap = None;
    }

    /// Whether a shape is part-way through being drawn.
    pub fn active(&self) -> bool {
        self.draft.is_some()
    }
}

/// Turns clicks into a [`Draft`], and a finished draft into an [`Edit`].
///
/// Reads the pointer's cell and does nothing at all when there is none, so a click that
/// happened outside the window never places a vertex.
///
/// A finish below the shape's minimum leaves the draft standing and says why, rather than
/// discarding vertices the GM drew. An id is taken from the document only once a draft has
/// yielded a feature, because asking for one marks the document unsaved and a refused add
/// would leave it unsaved with nothing changed.
pub fn draw_features(
    buttons: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    pointer: Res<MapPointer>,
    active: Res<ActiveTool>,
    over_ui: Res<super::PointerOverUi>,
    mut drafting: ResMut<Drafting>,
    mut doc: ResMut<WorldDoc>,
    mut selection: ResMut<Selection>,
    mut status: ResMut<StatusMessage>,
) {
    let Some(cell) = pointer.cell else {
        drafting.snap = None;
        return;
    };

    let drafted: Vec<_> = drafting
        .draft
        .as_ref()
        .map(|draft| draft.vertices().to_vec())
        .unwrap_or_default();
    let snap = {
        let cells_per_pixel = pointer.cells_per_pixel;
        let areas = &doc.areas;
        let world = doc.document.world();
        let drawn = |id| campaign::lod::is_pickable(world, areas, cells_per_pixel, id, &[]);
        pick::snap(world, &drafted, cell, pointer.slack(SNAP_SLACK_PIXELS), &drawn)
    };
    drafting.snap = Some(snap);

    if keys.just_pressed(KeyCode::Backspace)
        && let Some(draft) = drafting.draft.as_mut()
    {
        draft.pop();
    }

    let finishing = keys.just_pressed(KeyCode::Enter) || keys.just_pressed(KeyCode::NumpadEnter);
    let pressed = buttons.just_pressed(MouseButton::Left) && !over_ui.0;

    if active.shape == DraftShape::Point {
        if pressed {
            place(&mut doc, &mut selection, &mut status, {
                let mut draft = Draft::new(active.kind(), DraftShape::Point);
                draft.push(snap.at);
                draft
            });
        }
        return;
    }

    if pressed {
        let closing = snap.closes
            || drafting
                .draft
                .as_ref()
                .and_then(|draft| draft.vertices().last().copied())
                .is_some_and(|last| near(last, cell, pointer.slack(PICK_SLACK_PIXELS)));

        if closing && drafting.active() {
            finish(&mut drafting, &mut doc, &mut selection, &mut status);
            return;
        }

        drafting
            .draft
            .get_or_insert_with(|| Draft::new(active.kind(), active.shape))
            .push(snap.at);
        return;
    }

    if finishing && drafting.active() {
        finish(&mut drafting, &mut doc, &mut selection, &mut status);
    }
}

fn finish(
    drafting: &mut Drafting,
    doc: &mut WorldDoc,
    selection: &mut Selection,
    status: &mut StatusMessage,
) {
    let Some(draft) = drafting.draft.as_ref() else {
        return;
    };
    if !draft.can_finish() {
        status.say(format!(
            "a {} needs {} vertices, and this one has {}",
            draft.shape.shape(),
            draft.shape.least(),
            draft.len()
        ));
        return;
    }
    let Some(draft) = drafting.draft.take() else {
        return;
    };
    drafting.snap = None;
    place(doc, selection, status, draft);
}

fn place(
    doc: &mut WorldDoc,
    selection: &mut Selection,
    status: &mut StatusMessage,
    draft: Draft,
) {
    let Some(feature) = draft.finish() else {
        return;
    };
    let id = doc.document.fresh_id();
    let edit = gesture::place(doc.document.world(), id, feature);
    if doc::apply(doc, status, edit) {
        selection.replace_with(id);
    }
}

fn near(from: campaign::CellPoint, to: campaign::CellPoint, slack: f32) -> bool {
    (to.x - from.x).powi(2) + (to.y - from.y).powi(2) <= slack * slack
}

#[cfg(test)]
mod tests {
    use super::*;
    use campaign::feature::{FeatureKind, Geometry};
    use campaign::{Document, World};

    use crate::features::PointerOverUi;
    use crate::features::tool::Tool;

    fn at(x: f32, y: f32) -> campaign::CellPoint {
        campaign::CellPoint::new(x, y)
    }

    fn app() -> App {
        let mut app = App::new();
        app.init_resource::<ButtonInput<MouseButton>>()
            .init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<Drafting>()
            .init_resource::<Selection>()
            .init_resource::<PointerOverUi>()
            .init_resource::<StatusMessage>()
            .insert_resource(ActiveTool {
                tool: Tool::Draw,
                shape: DraftShape::Point,
                ..default()
            })
            .insert_resource(MapPointer {
                cell: Some(at(0.0, 0.0)),
                cells_per_pixel: 1.0,
            })
            .insert_resource(WorldDoc::new(Document::new(World::default())))
            .add_systems(Update, draw_features);
        app
    }

    fn shape(app: &mut App, shape: DraftShape, kind: FeatureKind) {
        let mut active = app.world_mut().resource_mut::<ActiveTool>();
        active.shape = shape;
        match shape {
            DraftShape::Point => active.point_kind = kind,
            DraftShape::Polyline => active.polyline_kind = kind,
            DraftShape::Polygon => active.polygon_kind = kind,
        }
    }

    fn click(app: &mut App, cell: campaign::CellPoint) {
        app.world_mut().resource_mut::<MapPointer>().cell = Some(cell);
        {
            let mut buttons = app.world_mut().resource_mut::<ButtonInput<MouseButton>>();
            buttons.press(MouseButton::Left);
        }
        app.update();
        let mut buttons = app.world_mut().resource_mut::<ButtonInput<MouseButton>>();
        buttons.release(MouseButton::Left);
        buttons.clear();
    }

    fn key(app: &mut App, code: KeyCode) {
        app.world_mut().resource_mut::<ButtonInput<KeyCode>>().press(code);
        app.update();
        let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
        keys.release(code);
        keys.clear();
    }

    fn world_of(app: &App) -> &World {
        app.world().resource::<WorldDoc>().document.world()
    }

    // One click with the point tool is one feature and one undo entry — the smallest whole
    // authoring gesture there is.
    #[test]
    fn a_click_with_the_point_tool_places_one_feature() {
        let mut app = app();
        click(&mut app, at(12.0, 8.0));

        assert_eq!(world_of(&app).len(), 1);
        assert_eq!(app.world().resource::<WorldDoc>().document.undo_depth(), 1);
        assert_eq!(app.world().resource::<Selection>().features.len(), 1);
    }

    // A polyline is drawn click by click and finished with Enter, and nothing reaches the
    // document until it is — the draft is the only place a one-vertex line exists.
    #[test]
    fn a_polyline_reaches_the_document_only_when_it_is_finished() {
        let mut app = app();
        shape(&mut app, DraftShape::Polyline, FeatureKind::Road);

        click(&mut app, at(0.0, 0.0));
        assert_eq!(world_of(&app).len(), 0, "a draft is not a feature");
        click(&mut app, at(50.0, 0.0));
        assert_eq!(world_of(&app).len(), 0);

        key(&mut app, KeyCode::Enter);
        assert_eq!(world_of(&app).len(), 1);
        assert!(app.world().resource::<Drafting>().draft.is_none());
    }

    // Finishing below the shape's minimum leaves the draft standing and says so, rather
    // than throwing away the vertices the GM drew.
    #[test]
    fn finishing_too_early_keeps_the_draft_and_says_why() {
        let mut app = app();
        shape(&mut app, DraftShape::Polygon, FeatureKind::Territory);

        click(&mut app, at(0.0, 0.0));
        click(&mut app, at(50.0, 0.0));
        key(&mut app, KeyCode::Enter);

        assert_eq!(world_of(&app).len(), 0);
        assert!(app.world().resource::<Drafting>().active(), "the draft survives");
        assert!(!app.world().resource::<StatusMessage>().0.is_empty());
    }

    // Backspace takes the last vertex back off while drawing — the fix for a misplaced
    // click, and the reason a draft is mutable at all.
    #[test]
    fn backspace_takes_the_last_vertex_off_the_draft() {
        let mut app = app();
        shape(&mut app, DraftShape::Polyline, FeatureKind::Road);

        click(&mut app, at(0.0, 0.0));
        click(&mut app, at(50.0, 0.0));
        click(&mut app, at(90.0, 0.0));
        key(&mut app, KeyCode::Backspace);

        let drafting = app.world().resource::<Drafting>();
        assert_eq!(drafting.draft.as_ref().unwrap().len(), 2);
    }

    // A point placed inside a settlement is parented to it in the same edit, which is what
    // makes the tavern belong to Riverford without the GM saying so twice.
    #[test]
    fn a_point_placed_in_a_settlement_is_parented_to_it() {
        let mut app = app();
        shape(&mut app, DraftShape::Polygon, FeatureKind::Settlement);
        for corner in [at(0.0, 0.0), at(100.0, 0.0), at(100.0, 100.0)] {
            click(&mut app, corner);
        }
        key(&mut app, KeyCode::Enter);
        let city = world_of(&app).features().next().map(|(id, _)| id).unwrap();

        shape(&mut app, DraftShape::Point, FeatureKind::Poi);
        click(&mut app, at(60.0, 20.0));

        let tavern = world_of(&app)
            .features()
            .find(|(_, feature)| matches!(feature.geometry, Geometry::Point(_)))
            .expect("the point is there");
        assert_eq!(tavern.1.parent, Some(city));
    }

    // A press that begins over the UI is not a press on the map, so clicking the toolbar
    // never drops a vertex behind it.
    #[test]
    fn a_click_over_the_ui_places_nothing() {
        let mut app = app();
        app.world_mut().resource_mut::<PointerOverUi>().0 = true;
        click(&mut app, at(12.0, 8.0));

        assert_eq!(world_of(&app).len(), 0);
    }
}
