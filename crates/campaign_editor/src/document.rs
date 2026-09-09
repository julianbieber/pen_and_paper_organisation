//! The document on screen, the ones that are not, and the single place any of them is
//! written.
//!
//! It sits at the editor's root rather than under `features/` because both halves need it:
//! authoring changes the open document, and the map draws the backdrop it carries. Under
//! `features/` the chunk streamer would have to reach into the authoring half to find out
//! what to draw, which is the dependency `map/pointer.rs` is careful to state it does not
//! have.
//!
//! **A document is parked, never dropped.** Leaving one keeps it with its undo stack, its
//! dirty flag and where the camera was looking at it, so a round trip costs the GM neither
//! their unsaved painting nor their history, and coming back to a dungeon returns to the
//! view they left it at. It is also why the close guard asks about every document rather
//! than the one on screen: quitting from a clean dungeon must not silently discard the
//! world map's unsaved edits, which always include the link to the dungeon being stood in.

use std::collections::HashMap;
use std::path::PathBuf;

use bevy::prelude::*;
use campaign::edit::{Edit, EditError};
use campaign::feature::FeatureId;
use campaign::lod::Areas;
use campaign::world::WorldError;
use campaign::Document;

use crate::StatusMessage;
use crate::map::backdrop::CameraBookmark;

/// Whether there is a document to author, decided once.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorldOutcome {
    Ready,
    Unavailable,
}

/// What became of the attempt to open the world document.
///
/// Its presence is what stops [`open_world`](crate::features::doc::open_world) running
/// again, so it is inserted on the refusing path as much as on the succeeding one. Without
/// it a `world.ron` that fails to load is re-read off disk and re-reported every frame for
/// the life of the process.
#[derive(Resource, Debug, Clone)]
pub struct WorldState {
    pub outcome: WorldOutcome,
    pub message: String,
}

/// A document that is open but not on screen, with everything that makes it resumable.
#[derive(Debug)]
pub struct ParkedDocument {
    pub document: Document,
    pub areas: Areas,
    /// Where the camera was looking at it, or `None` if it has never been on screen.
    pub camera: Option<CameraBookmark>,
}

/// The document being authored, where it is written, and every document parked behind it.
///
/// The path is a field rather than a second resource because [`WorldDoc::save`] is the only
/// place in the editor that writes a document, and that claim is checkable at one call site
/// only if the site knows on its own where to write. A separate resource would leave every
/// caller free to disagree with it — and there are two callers, one of them the prompt shown
/// when the window is closing.
///
/// The areas are session state rather than part of the document, for the reason the undo
/// stack is: an area is derivable from the features, and a cache written into the file would
/// be a second copy of a fact that can disagree with the first. It lives here rather than in
/// its own resource because it must be rebuilt in lockstep with the document — a resource
/// watched with `is_changed` would answer from last frame's geometry, and the hit tests read
/// it the same frame an edit lands.
#[derive(Resource, Debug)]
pub struct WorldDoc {
    pub document: Document,
    pub areas: Areas,
    /// Where the document on screen is read from and written to.
    pub path: PathBuf,
    /// The dungeon entry the document on screen belongs to, or `None` for the world map.
    pub entry: Option<FeatureId>,
    /// Every document that has been open this session and is not on screen, by path.
    parked: HashMap<PathBuf, ParkedDocument>,
}

impl WorldDoc {
    /// The campaign's world map under authorship, with its areas measured.
    pub fn world_map(document: Document, path: PathBuf) -> Self {
        let areas = Areas::of(document.world());
        Self {
            document,
            areas,
            path,
            entry: None,
            parked: HashMap::new(),
        }
    }

    /// Whether a dungeon is the document on screen.
    pub fn in_a_dungeon(&self) -> bool {
        self.entry.is_some()
    }

    /// Whether anything live is unsaved — on screen or parked.
    ///
    /// What the close guard asks.
    pub fn anything_unsaved(&self) -> bool {
        self.document.is_dirty()
            || self
                .parked
                .values()
                .any(|parked| parked.document.is_dirty())
    }

    /// Whether a document at `path` has already been open this session.
    pub fn is_parked(&self, path: &PathBuf) -> bool {
        self.parked.contains_key(path)
    }

    /// Put the document at `path` on screen, parking what was there and remembering
    /// `camera` against it.
    ///
    /// If that document is already parked it is resumed exactly as it was left — its undo
    /// stack, its unsaved changes and the view it was left at. Otherwise `fresh` is asked
    /// for it, which is where a load from disk or the creation of a new dungeon happens.
    ///
    /// Hands back where the camera should go, or `None` to frame the backdrop afresh.
    pub fn switch_to(
        &mut self,
        path: PathBuf,
        entry: Option<FeatureId>,
        camera: Option<CameraBookmark>,
        fresh: impl FnOnce() -> Document,
    ) -> Option<CameraBookmark> {
        let resumed = self.parked.remove(&path);
        let restore = resumed.as_ref().and_then(|parked| parked.camera);
        let (document, areas) = match resumed {
            Some(parked) => (parked.document, parked.areas),
            None => {
                let document = fresh();
                let areas = Areas::of(document.world());
                (document, areas)
            }
        };

        let leaving = ParkedDocument {
            document: std::mem::replace(&mut self.document, document),
            areas: std::mem::replace(&mut self.areas, areas),
            camera,
        };
        let was = std::mem::replace(&mut self.path, path);
        self.parked.insert(was, leaving);
        self.entry = entry;
        restore
    }

    /// Write the document on screen to the path it came from.
    ///
    /// The only place in the editor that writes the open document, which is what makes
    /// "written only by an explicit save" checkable at a single call site — and what keeps
    /// a save issued while a dungeon is open from landing on `world.ron`.
    pub fn save(&mut self) -> Result<(), WorldError> {
        self.document.save(&self.path)
    }

    /// Write every parked document that is unsaved.
    ///
    /// Separate from [`WorldDoc::save`] because only the prompt shown when the window is
    /// closing has any business writing a document the GM is not looking at. Stops at the
    /// first failure, naming it, so nothing is reported as saved that was not.
    pub fn save_parked(&mut self) -> Result<(), WorldError> {
        for (path, parked) in self.parked.iter_mut() {
            if parked.document.is_dirty() {
                parked.document.save(path)?;
            }
        }
        Ok(())
    }
}

/// Apply `edit` to the document on screen, reporting a refusal rather than swallowing it.
///
/// Returns whether it applied. Every authoring system routes through here: an edit refused
/// silently looks exactly like a click that never landed, and the GM has no way to tell
/// which happened.
///
/// The area cache is rebuilt only for an edit that could have moved a vertex. A brush
/// stroke changes no geometry, and re-running the shoelace sum of every polygon on each one
/// would make painting cost more the more the GM has drawn.
pub fn apply(doc: &mut WorldDoc, status: &mut StatusMessage, edit: Edit) -> bool {
    let geometric = moves_geometry(&edit);
    match doc.document.apply(edit) {
        Ok(()) => {
            if geometric {
                doc.areas.rebuild(doc.document.world());
            }
            true
        }
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

fn moves_geometry(edit: &Edit) -> bool {
    match edit {
        Edit::Add { .. }
        | Edit::Delete { .. }
        | Edit::MoveVertex { .. }
        | Edit::InsertVertex { .. }
        | Edit::RemoveVertex { .. } => true,
        Edit::SetKind { .. }
        | Edit::SetLabel { .. }
        | Edit::SetNote { .. }
        | Edit::SetParent { .. }
        | Edit::SetRank { .. }
        | Edit::SetMaxCellsPerPixel { .. }
        | Edit::SetDungeon { .. }
        | Edit::PaintTiles { .. } => false,
        Edit::Batch(edits) => edits.iter().any(moves_geometry),
    }
}
