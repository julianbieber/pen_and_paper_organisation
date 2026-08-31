//! Undo, redo, save, delete and escape.
//!
//! Every one of these is refused while a text field has keyboard focus. The property
//! panel's label field is on screen whenever something is selected, and raw key input
//! ignores focus — so without the gate, typing a settlement's name would delete it on the
//! first Delete, unwind the document on Ctrl+Z, and pop a draft vertex on every Backspace.

use bevy::prelude::*;
use campaign::edit::Edit;
use campaign::feature::Geometry;
use campaign::gesture;

use crate::features::doc::{self, WorldDoc};
use crate::features::draw::Drafting;
use crate::features::prompt::{Answer, Asking, Question};
use crate::features::select::{Dragging, Selection};
use crate::{OpenCampaign, StatusMessage};

/// Undo, redo, save, delete and escape.
///
/// A held drag is cancelled before any of them acts: undoing under one would rewind the
/// world the drag's start position refers to, and its release would then move a vertex
/// that is no longer the one that was grabbed.
///
/// Delete removes the selected vertex when one is selected and the selected features
/// otherwise, except on a point — whose one vertex *is* the feature, so the feature goes.
/// Deleting a feature something outside the selection hangs off raises a question instead
/// of acting.
pub fn authoring_keys(
    keys: Res<ButtonInput<KeyCode>>,
    campaign: Res<OpenCampaign>,
    mut doc: ResMut<WorldDoc>,
    mut selection: ResMut<Selection>,
    mut drafting: ResMut<Drafting>,
    mut dragging: ResMut<Dragging>,
    mut asking: ResMut<Asking>,
    mut status: ResMut<StatusMessage>,
) {
    let control = keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight);
    let shift = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);

    let acting = keys.just_pressed(KeyCode::KeyZ)
        || keys.just_pressed(KeyCode::KeyS)
        || keys.just_pressed(KeyCode::Delete)
        || keys.just_pressed(KeyCode::Escape);

    if acting && dragging.what.is_some() {
        dragging.cancel();
    }

    if control && keys.just_pressed(KeyCode::KeyZ) {
        let stepped = if shift {
            doc.document.redo()
        } else {
            doc.document.undo()
        };
        match stepped {
            Ok(true) => {}
            Ok(false) => status.say(if shift {
                "nothing to redo"
            } else {
                "nothing to undo"
            }),
            Err(refusal) => doc::report(&mut status, &refusal),
        }
        return;
    }

    if control && keys.just_pressed(KeyCode::KeyS) {
        match doc.save(&campaign.0) {
            Ok(()) => status.say(format!("saved {} feature(s)", doc.document.world().len())),
            Err(error) => {
                error!("{error}");
                status.say(error.to_string());
            }
        }
        return;
    }

    if keys.just_pressed(KeyCode::Escape) {
        if drafting.active() {
            drafting.abandon();
        } else {
            selection.clear();
        }
        return;
    }

    if keys.just_pressed(KeyCode::Delete) {
        delete(&mut doc, &mut selection, &mut asking, &mut status);
    }
}

/// Escape answers a standing question with Cancel.
///
/// Its own system because [`authoring_keys`] does not run while a question is up — which
/// is what stops the map changing under an answer — and a modal that can only be
/// dismissed with the mouse is one the GM has to reach for.
pub fn escape_answers_a_question(
    keys: Res<ButtonInput<KeyCode>>,
    mut asking: ResMut<Asking>,
    mut doc: ResMut<WorldDoc>,
    mut selection: ResMut<Selection>,
    campaign: Res<OpenCampaign>,
    mut status: ResMut<StatusMessage>,
    mut exit: MessageWriter<AppExit>,
) {
    if !keys.just_pressed(KeyCode::Escape) {
        return;
    }
    crate::features::prompt::answer(
        Answer::Cancel,
        &mut asking,
        &mut doc,
        &mut selection,
        &campaign,
        &mut status,
        &mut exit,
    );
}

fn delete(
    doc: &mut WorldDoc,
    selection: &mut Selection,
    asking: &mut Asking,
    status: &mut StatusMessage,
) {
    if let Some(vertex) = selection.vertex {
        let is_point = doc
            .document
            .world()
            .feature(vertex.feature)
            .is_some_and(|feature| matches!(feature.geometry, Geometry::Point(_)));

        let edit = if is_point {
            Edit::Delete { id: vertex.feature }
        } else {
            Edit::RemoveVertex {
                id: vertex.feature,
                index: vertex.index,
            }
        };
        if doc::apply(doc, status, edit) {
            selection.vertex = None;
        }
        return;
    }

    if selection.is_empty() {
        return;
    }

    let asked = gesture::outside_children(doc.document.world(), &selection.features);
    if let Some((id, _)) = asked.first() {
        asking.raise(Question::OrphansOnDelete, Some(*id));
        return;
    }

    let edit = gesture::remove_all(doc.document.world(), &selection.features);
    if doc::apply(doc, status, edit) {
        selection.clear();
    }
}
