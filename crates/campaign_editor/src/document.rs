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

    /// Write the document on screen, when it is dirty, and every parked one that is.
    ///
    /// Shared by the unsaved-close prompt and a sync, both of which write everything
    /// open rather than only what is on screen. Written only where dirty, matching
    /// [`WorldDoc::save_parked`] — a sync that wrote a clean document anyway would
    /// materialize a `world.ron` that was legitimately absent and commit it as though
    /// something had changed.
    pub fn save_everything(&mut self) -> Result<(), WorldError> {
        if self.document.is_dirty() {
            self.save()?;
        }
        self.save_parked()
    }

    /// Replace every document `touched` names with what is now on disk, clearing its undo
    /// history — an `Edit`'s inverse against a document a pull just replaced means
    /// nothing.
    ///
    /// A document whose file cannot be read, or whose reload would change whether it
    /// carries a grid — a dungeon must still carry one, the world map must not grow one —
    /// is refused: what is on screen stays, and the refusal is named on
    /// [`Reloaded::refused`] rather than silently discarded. A parked document that is
    /// reloaded keeps its camera bookmark.
    pub fn reload_from_disk(&mut self, touched: impl Fn(&std::path::Path) -> bool) -> Reloaded {
        let mut reloaded = Reloaded::default();

        if touched(&self.path) {
            match Self::reload_one(&self.path, &self.document) {
                Ok(fresh) => {
                    self.areas = Areas::of(fresh.world());
                    self.document = fresh;
                    reloaded.on_screen = true;
                    reloaded.paths.push(self.path.clone());
                }
                Err(reason) => reloaded.refused.push(reason),
            }
        }

        for (path, parked) in self.parked.iter_mut() {
            if !touched(path) {
                continue;
            }
            match Self::reload_one(path, &parked.document) {
                Ok(fresh) => {
                    parked.areas = Areas::of(fresh.world());
                    parked.document = fresh;
                    reloaded.paths.push(path.clone());
                }
                Err(reason) => reloaded.refused.push(reason),
            }
        }

        reloaded
    }

    fn reload_one(path: &std::path::Path, current: &Document) -> Result<Document, String> {
        let fresh = Document::load(path).map_err(|error| error.to_string())?;
        if fresh.world().grid().is_some() != current.world().grid().is_some() {
            return Err(format!(
                "`{}` changed whether it carries a grid, so the reload was refused",
                path.display()
            ));
        }
        Ok(fresh)
    }
}

/// What [`WorldDoc::reload_from_disk`] did.
#[derive(Debug, Default, Clone)]
pub struct Reloaded {
    /// Whether the document on screen was replaced.
    pub on_screen: bool,
    /// Every path — on screen or parked — that was replaced.
    pub paths: Vec<PathBuf>,
    /// Why a touched document was left as it was, one line per refusal.
    pub refused: Vec<String>,
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
        | Edit::PaintTiles { .. }
        | Edit::SetImage { .. }
        | Edit::PlaceImage { .. }
        | Edit::SetImageOpacity { .. } => false,
        Edit::Batch(edits) => edits.iter().any(moves_geometry),
    }
}

#[cfg(test)]
mod tests {
    use campaign::edit::Edit;
    use campaign::feature::{CellPoint, Feature, FeatureKind, Geometry};
    use campaign::grid::{DEFAULT_GRID_CELLS, DEFAULT_METRES_PER_CELL, TileGrid};
    use campaign::world::World;

    use super::*;

    fn point_feature(label: &str) -> Feature {
        Feature {
            label: label.to_owned(),
            ..Feature::plain(FeatureKind::Poi, Geometry::Point(CellPoint::new(1.0, 1.0)))
        }
    }

    fn world_with_one_feature(label: &str) -> World {
        let mut document = Document::new(World::default());
        let id = document.fresh_id();
        document
            .apply(Edit::Add {
                id,
                feature: point_feature(label),
            })
            .expect("apply");
        document.world().clone()
    }

    fn default_grid() -> TileGrid {
        TileGrid::new(DEFAULT_GRID_CELLS, DEFAULT_GRID_CELLS, DEFAULT_METRES_PER_CELL)
            .expect("the default extent and scale are legal by construction")
    }

    // A sync must not materialize a `world.ron` that was legitimately absent just
    // because it saved everything open — an absent document is an empty world, and
    // saving a clean one anyway would turn that into a spurious commit.
    #[test]
    fn save_everything_writes_nothing_for_a_clean_on_screen_document() {
        let tmp = tempfile::tempdir().expect("temp dir");
        let path = tmp.path().join("world.ron");
        assert!(!path.exists());

        let mut doc = WorldDoc::world_map(Document::new(World::default()), path.clone());
        assert!(!doc.document.is_dirty());

        doc.save_everything().expect("save everything");

        assert!(!path.exists(), "a clean document must not be written");
    }

    // The whole point of a reload: a document a pull replaced comes back with the new
    // features, and its undo history is gone — an inverse against a document that is no
    // longer there means nothing.
    #[test]
    fn a_reloaded_document_carries_the_new_features_with_no_history() {
        let tmp = tempfile::tempdir().expect("temp dir");
        let path = tmp.path().join("world.ron");
        World::default().save(&path).expect("write");

        let mut doc = WorldDoc::world_map(Document::load(&path).expect("load"), path.clone());
        let id = doc.document.fresh_id();
        doc.document
            .apply(Edit::Add {
                id,
                feature: point_feature("before"),
            })
            .expect("apply");
        assert!(doc.document.is_dirty());
        assert_eq!(doc.document.undo_depth(), 1);

        world_with_one_feature("after").save(&path).expect("write");

        let reloaded = doc.reload_from_disk(|_| true);
        assert!(reloaded.on_screen);
        assert!(reloaded.refused.is_empty());
        assert!(!doc.document.is_dirty());
        assert_eq!(doc.document.undo_depth(), 0);
        let labels: Vec<&str> = doc
            .document
            .world()
            .features()
            .map(|(_, feature)| feature.label.as_str())
            .collect();
        assert_eq!(labels, vec!["after"]);
    }

    // A parked document is reloaded too, and keeps its camera bookmark.
    #[test]
    fn a_parked_document_touched_by_the_reload_is_replaced() {
        let tmp = tempfile::tempdir().expect("temp dir");
        let world_path = tmp.path().join("world.ron");
        let dungeon_path = tmp.path().join("crypt.ron");
        World::default().save(&world_path).expect("write world");
        Document::new(World::on_a_grid(default_grid()))
            .save(&dungeon_path)
            .expect("write dungeon");

        let mut doc = WorldDoc::world_map(Document::load(&world_path).expect("load"), world_path.clone());
        let bookmark = CameraBookmark {
            translation: Vec2::new(1.0, 2.0),
            scale: 3.0,
        };
        doc.switch_to(dungeon_path.clone(), None, Some(bookmark), || {
            Document::load(&dungeon_path).expect("load dungeon")
        });

        world_with_one_feature("arrived").save(&world_path).expect("write");

        let reloaded = doc.reload_from_disk(|path| path == world_path);
        assert!(!reloaded.on_screen, "the world map is parked, not on screen");
        assert_eq!(reloaded.paths, vec![world_path.clone()]);
        assert!(reloaded.refused.is_empty());

        let restore = doc.switch_to(world_path.clone(), None, None, || {
            unreachable!("the world map is parked")
        });
        assert_eq!(restore, Some(bookmark));
        let labels: Vec<&str> = doc
            .document
            .world()
            .features()
            .map(|(_, feature)| feature.label.as_str())
            .collect();
        assert_eq!(labels, vec!["arrived"]);
        assert_eq!(doc.document.undo_depth(), 0);
    }

    // A document `touched` says nothing about keeps its history — a reload is not asked
    // for by name alone.
    #[test]
    fn an_untouched_document_keeps_its_history() {
        let tmp = tempfile::tempdir().expect("temp dir");
        let path = tmp.path().join("world.ron");
        World::default().save(&path).expect("write");

        let mut doc = WorldDoc::world_map(Document::load(&path).expect("load"), path.clone());
        let id = doc.document.fresh_id();
        doc.document
            .apply(Edit::Add {
                id,
                feature: point_feature("mine"),
            })
            .expect("apply");

        let reloaded = doc.reload_from_disk(|_| false);
        assert!(!reloaded.on_screen);
        assert!(reloaded.paths.is_empty());
        assert_eq!(doc.document.undo_depth(), 1);
    }

    // A dungeon whose file lost its grid is refused rather than swapped in — the world
    // map must not grow one and a dungeon must not lose it.
    #[test]
    fn a_dungeon_whose_file_loses_its_grid_is_refused() {
        let tmp = tempfile::tempdir().expect("temp dir");
        let path = tmp.path().join("crypt.ron");
        Document::new(World::on_a_grid(default_grid()))
            .save(&path)
            .expect("write dungeon");

        let mut doc = WorldDoc::world_map(Document::load(&path).expect("load"), path.clone());
        World::default().save(&path).expect("overwrite without a grid");

        let reloaded = doc.reload_from_disk(|_| true);
        assert!(!reloaded.on_screen);
        assert_eq!(reloaded.refused.len(), 1);
        assert!(doc.document.world().grid().is_some(), "the in-memory document was kept");
    }
}
