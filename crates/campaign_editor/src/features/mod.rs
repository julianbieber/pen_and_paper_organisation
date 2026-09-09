//! The authoring systems, and the set they run in against the map's.
//!
//! The order is one chain, and every edge in it is load-bearing. The pointer's cell is
//! read against the camera this frame moved; the tool strip is queried on the frame it is
//! spawned, which needs the deferred-command flush an ordering edge inserts; a landed note
//! is applied ahead of everything that reads the document, so it reaches the panel, the
//! selection and the map on the frame it arrived rather than the frame after; and the
//! overlay draws after everything that could have changed what it draws.
//!
//! Two conditions are named here rather than restated per system, because "is the pointer
//! over the UI" and "is a question up" answered two ways would be two answers to one
//! question. The first is a *resource* rather than a run condition on purpose: a drag that
//! began on the map has to keep running when it crosses the tool strip, so the gate
//! belongs to the press inside the system, not to the system.

use bevy::input_focus::InputFocus;
use bevy::prelude::*;
use bevy::text::EditableText;

/// The campaign's `world.ron` under authorship.
pub mod doc;
/// Turning clicks into a draft, and a finished draft into an Edit.
pub mod draw;
/// Opening the dungeon a selected entry names, and getting back to the world map.
pub mod dungeon;
/// Undo, redo, save, delete and escape.
pub mod keys;
/// The selected feature's label, kind, rank, reveal scale, parent and note link.
pub mod panel;
/// Turning a brush stroke on a dungeon's grid into one Edit.
pub mod paint;
/// The fixed set of pens the map is stroked through, and the order they paint.
pub mod pens;
/// The two questions authoring has to ask, and the close it holds back.
pub mod prompt;
/// Drawing the features in view, the draft, the selection and its handles.
pub mod render;
/// What is selected, and carrying a drag until it lands as one Edit.
pub mod select;
/// Which tool is active, and which kind each drawing tool will place.
pub mod tool;

use crate::document::{WorldDoc, WorldState};
use crate::map::backdrop::Backdrop;
use crate::{EditorSet, OpenCampaign};

/// Whether the pointer is over a UI node, as a value rather than a run condition.
///
/// A run condition would stop `select_features` the moment a drag crossed the tool strip,
/// and the release would never be seen — leaving the drag held for ever. Carried as a
/// resource, the gate applies to the press alone.
#[derive(Resource, Debug, Default)]
pub struct PointerOverUi(pub bool);

/// Everything that authors features on top of the map.
pub struct FeaturesPlugin;

impl Plugin for FeaturesPlugin {
    fn build(&self, app: &mut App) {
        pens::register_pens(app);
        app.init_resource::<tool::ActiveTool>()
            .init_resource::<draw::Drafting>()
            .init_resource::<paint::Stroking>()
            .init_resource::<dungeon::DungeonIntent>()
            .init_resource::<select::Selection>()
            .init_resource::<select::Dragging>()
            .init_resource::<prompt::Asking>()
            .init_resource::<panel::PendingLabel>()
            .init_resource::<PointerOverUi>()
            .add_systems(
                Update,
                (
                    note_pointer_over_ui,
                    doc::open_world.run_if(
                        resource_exists::<OpenCampaign>.and_then(not(resource_exists::<WorldState>)),
                    ),
                    doc::show_world_state.run_if(resource_exists_and_changed::<WorldState>),
                    (
                        tool::build_tool_strip,
                        panel::build_property_panel,
                        prompt::build_prompt,
                    )
                        .run_if(resource_added::<WorldDoc>),
                    crate::notes::panel::build_notes_panel
                        .run_if(resource_added::<OpenCampaign>),
                    crate::notes::finish_note_job
                        .run_if(resource_exists::<WorldDoc>.and_then(
                            crate::notes::a_note_job_is_running,
                        )),
                    crate::notes::note_job_settled,
                    tool::sync_tool_strip.run_if(resource_exists_and_changed::<tool::ActiveTool>),
                    (
                        draw::draw_features.run_if(a_drawing_tool_is_active),
                        select::select_features.run_if(the_select_tool_is_active),
                        paint::paint_tiles
                            .run_if(paint::the_paint_tool_is_active.and_then(paint::a_grid_is_open)),
                        keys::authoring_keys,
                        panel::commit_label,
                    )
                        .run_if(authoring_is_live),
                    (
                        dungeon::switch_document.run_if(
                            dungeon::a_switch_was_asked_for
                                .and_then(resource_exists::<WorldDoc>)
                                .and_then(resource_exists::<Backdrop>),
                        ),
                        keys::escape_answers_a_question.run_if(prompt::a_question_is_up),
                    )
                        .chain(),
                    select::reconcile_selection.run_if(resource_exists::<WorldDoc>),
                    panel::show_properties.run_if(
                        resource_exists::<WorldDoc>.and_then(
                            resource_changed::<select::Selection>
                                .or_else(resource_changed::<WorldDoc>),
                        ),
                    ),
                    (
                        crate::notes::watch::drain_notes_watch
                            .run_if(crate::notes::watch::the_notebook_changed),
                        crate::notes::references::follow_selection.run_if(
                            resource_exists::<WorldDoc>.and_then(
                                resource_changed::<select::Selection>
                                    .or_else(resource_changed::<WorldDoc>),
                            ),
                        ),
                        crate::notes::references::run_reference_query.run_if(
                            resource_exists::<OpenCampaign>
                                .and_then(crate::notes::references::a_reference_query_has_work),
                        ),
                    )
                        .chain(),
                    panel::show_note_buttons.run_if(
                        resource_exists::<WorldDoc>.and_then(
                            resource_changed::<select::Selection>
                                .or_else(resource_changed::<WorldDoc>)
                                .or_else(resource_changed::<crate::notes::NoteJob>)
                                .or_else(resource_changed::<crate::notes::ZkState>),
                        ),
                    ),
                    crate::notes::panel::show_new_note_buttons.run_if(
                        resource_changed::<crate::notes::NoteJob>
                            .or_else(resource_changed::<crate::notes::ZkState>)
                            .or_else(resource_changed::<crate::notes::NoteTitle>),
                    ),
                    crate::notes::references::show_references.run_if(
                        resource_changed::<crate::notes::references::References>
                            .or_else(resource_changed::<crate::notes::ZkState>)
                            .or_else(resource_added::<OpenCampaign>),
                    ),
                    prompt::guard_close.run_if(resource_exists::<WorldDoc>),
                    prompt::show_prompt.run_if(resource_exists_and_changed::<prompt::Asking>),
                    pens::size_pens.run_if(resource_exists::<crate::map::load::MapAssets>),
                    render::render_features.run_if(
                        resource_exists::<WorldDoc>.and_then(resource_exists::<Backdrop>),
                    ),
                )
                    .chain()
                    .in_set(EditorSet::Authoring)
                    .after(EditorSet::Map),
            );
    }
}

fn authoring_is_live(
    doc: Option<Res<WorldDoc>>,
    terrain: Option<Res<Backdrop>>,
    asking: Option<Res<prompt::Asking>>,
    focus: Res<InputFocus>,
    fields: Query<(), With<EditableText>>,
) -> bool {
    doc.is_some()
        && terrain.is_some()
        && asking.is_none_or(|asking| asking.question.is_none())
        && focus.get().is_none_or(|entity| !fields.contains(entity))
}

fn a_drawing_tool_is_active(active: Res<tool::ActiveTool>) -> bool {
    active.drawing()
}

fn the_select_tool_is_active(active: Res<tool::ActiveTool>) -> bool {
    active.selecting()
}

fn note_pointer_over_ui(
    hovered: Res<bevy::picking::hover::HoverMap>,
    nodes: Query<(), With<Node>>,
    mut over: ResMut<PointerOverUi>,
) {
    let now = hovered
        .values()
        .flat_map(|hits| hits.keys())
        .any(|entity| nodes.contains(*entity));
    if over.0 != now {
        over.0 = now;
    }
}
