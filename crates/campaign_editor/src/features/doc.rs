//! Opening the campaign's world map, and saying why there is none.
//!
//! What a document *is* lives at the editor's root, in [`crate::document`], because the map
//! half needs it too. What is left here is the pair of systems that put the world map on
//! screen when a campaign opens — which is authoring's business and not the map's.

use bevy::prelude::*;
use campaign::{Document, layout};

use crate::document::{WorldDoc, WorldOutcome, WorldState};
use crate::{OpenCampaign, StatusMessage};

pub use crate::document::apply;

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
            commands.insert_resource(WorldDoc::world_map(document, path));
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
