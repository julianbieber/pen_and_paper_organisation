//! Driving the running editor from outside the process.
//!
//! The editor keeps its real window, its real systems and its real document; this is a
//! way to talk to it, not a second way to run it. Present only when `PNP_CONTROL` names
//! a socket, and inert otherwise.
//!
//! **Input is synthesised, not bypassed.** A `click` writes the pointer's cell and the
//! left button and then lets the ordinary authoring systems run, so it exercises exactly
//! the code a real click exercises. A verb that reached into the document directly would
//! prove nothing about the tool the GM uses — which is the same reason every change is an
//! [`Edit`](campaign::Edit) rather than a method call.
//!
//! **A command is synchronous from the client's side.** The reply is held until the
//! effect has actually happened — `click` answers once the frame carrying the press has
//! run, `capture` once the PNG is on disk — so a caller never sleeps and hopes. What
//! blocks is the *client*; the editor runs on undisturbed.

use bevy::prelude::*;

mod command;
mod server;

/// Installs the control server, if `PNP_CONTROL` names a socket to listen on.
///
/// Without the variable the plugin is inert and the editor behaves as though it were
/// absent — no socket, no systems, no cost.
pub struct ControlPlugin;

impl Plugin for ControlPlugin {
    fn build(&self, app: &mut App) {
        server::build(app);
    }
}
