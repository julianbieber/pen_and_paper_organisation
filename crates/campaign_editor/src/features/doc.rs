//! The campaign's `world.ron` under authorship: opened once with the campaign, changed
//! only by applying an [`Edit`], and written in exactly one place.
//!
//! Every authoring system goes through [`apply`] rather than reaching the [`Document`]
//! itself, because an edit that is refused and says nothing is indistinguishable from a
//! press that was not noticed — and there are five systems that could otherwise each
//! decide differently what to do about a refusal.

use bevy::prelude::*;
use campaign::edit::{Edit, EditError};
use campaign::world::WorldError;
use campaign::{Campaign, Document, layout};

use crate::{OpenCampaign, StatusMessage};

/// Whether there is a document to author, decided once.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorldOutcome {
    Ready,
    Unavailable,
}

/// What became of the attempt to open the world document.
///
/// Its presence is what stops [`open_world`] running again, so it is inserted on the
/// refusing path as much as on the succeeding one. Without it a `world.ron` that fails to
/// load is re-read off disk and re-reported every frame for the life of the process — the
/// same failure [`MapState`](crate::map::load::MapState) exists to avoid.
#[derive(Resource, Debug, Clone)]
pub struct WorldState {
    pub outcome: WorldOutcome,
    pub message: String,
}

/// The world being authored.
///
/// Holds only the [`Document`]. Where `world.ron` lives is not a field: the campaign
/// already says, through [`layout::world`], and a copy here would be a second answer to
/// the same question.
#[derive(Resource, Debug)]
pub struct WorldDoc {
    pub document: Document,
}

impl WorldDoc {
    /// Write the world to the campaign's `world.ron`.
    ///
    /// The only place in the editor that writes it, which is what makes "written only by
    /// an explicit save" checkable at a single call site.
    pub fn save(&mut self, campaign: &Campaign) -> Result<(), WorldError> {
        self.document.save(&layout::world(campaign.root()))
    }
}

/// Apply `edit`, reporting a refusal rather than swallowing it.
///
/// Returns whether it applied. Every authoring system routes through here: an edit
/// refused silently looks exactly like a click that never landed, and the GM has no way
/// to tell which happened.
pub fn apply(doc: &mut WorldDoc, status: &mut StatusMessage, edit: Edit) -> bool {
    match doc.document.apply(edit) {
        Ok(()) => true,
        Err(refusal) => {
            report(status, &refusal);
            false
        }
    }
}

/// Put a refused edit on the status line.
pub fn report(status: &mut StatusMessage, refusal: &EditError) {
    warn!("{refusal}");
    status.say(refusal.to_string());
}

/// Loads the campaign's `world.ron` the moment the campaign opens, and records the
/// outcome whichever way it went.
///
/// An absent file is an empty world rather than an error — creating a campaign writes
/// none, so a campaign that has never been authored opens ready to draw. A file that is
/// there and refused leaves no [`WorldDoc`], so the terrain still draws and nothing can
/// overwrite a document that was not understood.
pub fn open_world(mut commands: Commands, open: Res<OpenCampaign>) {
    let path = layout::world(open.0.root());
    match Document::load(&path) {
        Ok(document) => {
            info!(
                "opened {} feature(s) from {}",
                document.world().len(),
                path.display()
            );
            commands.insert_resource(WorldState {
                outcome: WorldOutcome::Ready,
                message: String::new(),
            });
            commands.insert_resource(WorldDoc { document });
        }
        Err(error) => {
            error!("{error}");
            commands.insert_resource(WorldState {
                outcome: WorldOutcome::Unavailable,
                message: error.to_string(),
            });
        }
    }
}

/// Says why there is no document to author, once.
///
/// The counterpart to [`show_map_state`](crate::map::load::show_map_state), and the same
/// discipline: the refusal is stated from the recorded outcome rather than at the moment
/// it happened, so it is said once rather than every frame something retries.
pub fn show_world_state(state: Res<WorldState>, mut status: ResMut<StatusMessage>) {
    if state.outcome == WorldOutcome::Unavailable && status.0 != state.message {
        status.0.clone_from(&state.message);
    }
}
