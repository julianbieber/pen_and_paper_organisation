//! The combat maps open this session, and which one, if any, is on screen.
//!
//! It sits at the editor's root beside `document.rs`, for that module's reason: the map
//! draws the grid of the combat map on screen and authoring paints it.
//!
//! A combat map sits over whatever [`WorldDoc`](crate::document::WorldDoc) has on screen
//! and leaves it untouched, so going back to the map returns to exactly that document. A
//! combat map that is not on screen is parked with its undo stack, its dirty flag and
//! where the camera was looking at it, as a parked world document is.
//!
//! An open map knows the file it is stored as from the moment it is opened, and is written
//! there only when it is saved. What the campaign's `combat` directory holds is cached here
//! as the last listing of it, and forgotten whenever the directory may have changed.

use std::path::Path;

use bevy::prelude::*;
use campaign::{CombatMap, Document, Tokens, WorldError, layout};

use crate::map::backdrop::CameraBookmark;

/// The most combat maps open at once — as many as the panel has rows for.
pub const MAX_OPEN_COMBAT_MAPS: usize = 8;

/// A combat map that is open this session, and where the camera was left on it.
#[derive(Debug)]
pub struct OpenCombatMap {
    pub document: Document<CombatMap>,
    /// The file name inside the campaign's `combat` directory it is saved as, whether or
    /// not that file has been written yet.
    pub file: String,
    /// Where the camera was looking at it when it was parked, or `None` if it never was.
    pub camera: Option<CameraBookmark>,
    /// The tokens on it this session. Never saved, and kept while the map is parked.
    pub tokens: Tokens,
}

/// Every combat map open this session, in the order they were opened, and which one is on
/// screen.
///
/// Session state: a map reaches the disk only through [`CombatMaps::save_on_screen`] and
/// [`CombatMaps::save_everything`], its tokens never do, and
/// [`crate::session::close_campaign`] resets it.
#[derive(Resource, Debug, Default)]
pub struct CombatMaps {
    maps: Vec<OpenCombatMap>,
    on_screen: Option<usize>,
    under: Option<CameraBookmark>,
    stored: Option<Vec<String>>,
}

impl CombatMaps {
    /// The combat map on screen, or `None` when the world document is.
    pub fn on_screen(&self) -> Option<&Document<CombatMap>> {
        self.on_screen.map(|index| &self.maps[index].document)
    }

    /// The combat map on screen, to apply an edit to.
    pub fn on_screen_mut(&mut self) -> Option<&mut Document<CombatMap>> {
        self.on_screen.map(|index| &mut self.maps[index].document)
    }

    /// The tokens on the combat map on screen, with that map's `(width, height)` in cells.
    pub fn tokens_on_screen(&self) -> Option<(&Tokens, (u32, u32))> {
        let open = &self.maps[self.on_screen?];
        let grid = open.document.content().grid();
        Some((&open.tokens, (grid.width(), grid.height())))
    }

    /// The tokens on the combat map on screen, to change, with that map's `(width, height)`
    /// in cells.
    pub fn tokens_on_screen_mut(&mut self) -> Option<(&mut Tokens, (u32, u32))> {
        let open = &mut self.maps[self.on_screen?];
        let grid = open.document.content().grid();
        let extent = (grid.width(), grid.height());
        Some((&mut open.tokens, extent))
    }

    /// How many tokens every open combat map carries, in the order it was opened.
    pub fn token_counts(&self) -> impl Iterator<Item = usize> {
        self.maps.iter().map(|open| open.tokens.len())
    }

    /// Whether a combat map is on screen.
    pub fn is_on_screen(&self) -> bool {
        self.on_screen.is_some()
    }

    /// Every open combat map in the order it was opened, with whether it is on screen.
    pub fn listed(&self) -> impl Iterator<Item = (&Document<CombatMap>, bool)> {
        self.maps
            .iter()
            .enumerate()
            .map(|(index, open)| (&open.document, self.on_screen == Some(index)))
    }

    /// The name of every open combat map, in the order it was opened.
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.maps.iter().map(|open| open.document.content().name())
    }

    /// The file name of every open combat map, in the order it was opened.
    pub fn files(&self) -> impl Iterator<Item = &str> {
        self.maps.iter().map(|open| open.file.as_str())
    }

    /// Whether any open combat map is unsaved, on screen or parked.
    pub fn anything_unsaved(&self) -> bool {
        self.maps.iter().any(|open| open.document.is_dirty())
    }

    /// Whether [`MAX_OPEN_COMBAT_MAPS`] are open, so another may not be.
    pub fn is_full(&self) -> bool {
        self.maps.len() >= MAX_OPEN_COMBAT_MAPS
    }

    /// Put `map` on screen as a new, clean document.
    ///
    /// `here` is where the camera is looking now: it is kept against the combat map being
    /// parked, or, when the world document is on screen, as where [`CombatMaps::leave`]
    /// returns to. `file` is the name inside `combat/` it saves as. The caller checks
    /// [`CombatMaps::is_full`] first, and that no open map already holds `file`; this
    /// refuses neither.
    pub fn open(&mut self, map: CombatMap, file: String, here: Option<CameraBookmark>) {
        self.park(here);
        self.maps.push(OpenCombatMap {
            document: Document::new(map),
            file,
            camera: None,
            tokens: Tokens::default(),
        });
        self.on_screen = Some(self.maps.len() - 1);
    }

    /// Put the open combat map called `name` on screen, parking what was there against
    /// `here`.
    ///
    /// Hands back where the camera was left on it, or `None` to frame it afresh. Refuses an
    /// unknown name with the sentence to show, having changed nothing.
    pub fn switch_to(
        &mut self,
        name: &str,
        here: Option<CameraBookmark>,
    ) -> Result<Option<CameraBookmark>, String> {
        let Some(index) = self
            .maps
            .iter()
            .position(|open| open.document.content().name() == name)
        else {
            return Err(format!("no open combat map is called {name}"));
        };
        Ok(self.show(index, here))
    }

    /// Put the open combat map saved as `file` on screen, exactly as
    /// [`CombatMaps::switch_to`] does for a name.
    pub fn switch_to_file(
        &mut self,
        file: &str,
        here: Option<CameraBookmark>,
    ) -> Result<Option<CameraBookmark>, String> {
        let Some(index) = self.maps.iter().position(|open| open.file == file) else {
            return Err(format!("no open combat map is saved as combat/{file}"));
        };
        Ok(self.show(index, here))
    }

    /// Remember `files` as what the `combat` directory holds.
    pub fn set_stored(&mut self, files: Vec<String>) {
        self.stored = Some(files);
    }

    /// Whether the `combat` directory has to be listed again before the panel can show it.
    pub fn needs_listing(&self) -> bool {
        self.stored.is_none()
    }

    /// Drop the remembered listing, so the directory is listed again.
    pub fn forget_stored(&mut self) {
        self.stored = None;
    }

    /// Every remembered stored file no open map is saved as, in listing order. Empty while
    /// nothing is remembered.
    pub fn stored_not_open(&self) -> Vec<String> {
        self.stored
            .iter()
            .flatten()
            .filter(|file| !self.maps.iter().any(|open| &open.file == *file))
            .cloned()
            .collect()
    }

    /// Save the combat map on screen into the campaign at `root`, and hand back the file
    /// name it was saved as.
    ///
    /// `None` when no combat map is on screen. A save that fails leaves the map unsaved, as
    /// [`Document::save`] does; one that lands forgets the stored listing.
    pub fn save_on_screen(&mut self, root: &Path) -> Option<Result<String, WorldError>> {
        let index = self.on_screen?;
        let open = &mut self.maps[index];
        let saved = open
            .document
            .save(&layout::combat_map(root, &open.file))
            .map(|()| open.file.clone());
        if saved.is_ok() {
            self.forget_stored();
        }
        Some(saved)
    }

    /// Save every unsaved combat map, on screen or parked, into the campaign at `root`.
    ///
    /// Stops at the first that fails and returns its error; the maps before it stay saved
    /// and it and those after it stay unsaved. The stored listing is forgotten when anything
    /// was written.
    pub fn save_everything(&mut self, root: &Path) -> Result<(), WorldError> {
        let mut wrote = false;
        let mut outcome = Ok(());
        for open in self.maps.iter_mut().filter(|open| open.document.is_dirty()) {
            if let Err(error) = open.document.save(&layout::combat_map(root, &open.file)) {
                outcome = Err(error);
                break;
            }
            wrote = true;
        }
        if wrote {
            self.forget_stored();
        }
        outcome
    }

    /// Park the combat map on screen against `here` and go back to the world document.
    ///
    /// `None` when no combat map is on screen, having changed nothing. Otherwise hands back
    /// where the camera was on the world document when the first combat map was opened over
    /// it, which is itself `None` when that was not known.
    pub fn leave(&mut self, here: Option<CameraBookmark>) -> Option<Option<CameraBookmark>> {
        let index = self.on_screen.take()?;
        self.maps[index].camera = here;
        Some(self.under.take())
    }

    fn show(&mut self, index: usize, here: Option<CameraBookmark>) -> Option<CameraBookmark> {
        self.park(here);
        self.on_screen = Some(index);
        self.maps[index].camera
    }

    fn park(&mut self, here: Option<CameraBookmark>) {
        match self.on_screen {
            Some(index) => self.maps[index].camera = here,
            None => self.under = here,
        }
    }
}

/// Whether a combat map is on screen.
pub fn a_combat_map_is_on_screen(maps: Option<Res<CombatMaps>>) -> bool {
    maps.is_some_and(|maps| maps.is_on_screen())
}

/// Whether the world document, rather than a combat map, is on screen.
pub fn the_map_is_on_screen(maps: Option<Res<CombatMaps>>) -> bool {
    !a_combat_map_is_on_screen(maps)
}

#[cfg(test)]
mod tests {
    use campaign::brush::TileChange;
    use campaign::{CombatEdit, CombatTile};

    use super::*;

    fn bookmark(x: f32) -> Option<CameraBookmark> {
        Some(CameraBookmark {
            translation: Vec2::new(x, 0.0),
            scale: 1.0,
        })
    }

    fn painted(name: &str) -> CombatMap {
        let mut map = CombatMap::new(name, 8, 8).unwrap();
        CombatEdit::PaintTiles {
            changes: vec![TileChange { x: 1, y: 1, tile: CombatTile::Tree }],
        }
        .apply(&mut map)
        .unwrap();
        map
    }

    fn paint_on_screen(maps: &mut CombatMaps) {
        maps.on_screen_mut()
            .unwrap()
            .apply(CombatEdit::PaintTiles {
                changes: vec![TileChange { x: 2, y: 2, tile: CombatTile::Mud }],
            })
            .unwrap();
    }

    // The second acceptance criterion: going to the map and back keeps the painting, its
    // undo history and its unsaved flag.
    #[test]
    fn a_parked_map_keeps_its_history_across_a_round_trip() {
        let mut maps = CombatMaps::default();
        maps.open(painted("Ford"), "ford.ron".to_owned(), bookmark(1.0));
        paint_on_screen(&mut maps);

        assert_eq!(maps.leave(bookmark(2.0)), Some(bookmark(1.0)));
        assert!(!maps.is_on_screen());
        assert_eq!(maps.switch_to("Ford", bookmark(3.0)), Ok(bookmark(2.0)));

        let document = maps.on_screen().unwrap();
        assert_eq!(document.undo_depth(), 1);
        assert!(document.is_dirty());
        assert_eq!(document.content().grid().get(1, 1), Some(CombatTile::Tree));
        assert_eq!(document.content().grid().get(2, 2), Some(CombatTile::Mud));
    }

    // Leaving returns the camera to where it was on the document underneath, not to
    // wherever it was on another combat map in between.
    #[test]
    fn leaving_restores_the_camera_of_the_document_underneath() {
        let mut maps = CombatMaps::default();
        maps.open(painted("Ford"), "ford.ron".to_owned(), bookmark(1.0));
        maps.open(painted("Bridge"), "bridge.ron".to_owned(), bookmark(5.0));
        assert_eq!(maps.leave(bookmark(6.0)), Some(bookmark(1.0)));
        assert_eq!(maps.leave(bookmark(7.0)), None);
    }

    // The close guard asks about a map nobody is looking at: a parked, painted map is lost
    // just as surely on quit as the one on screen.
    #[test]
    fn a_parked_dirty_map_counts_as_unsaved() {
        let mut maps = CombatMaps::default();
        assert!(!maps.anything_unsaved());
        maps.open(painted("Ford"), "ford.ron".to_owned(), None);
        assert!(!maps.anything_unsaved(), "a new map is clean");
        paint_on_screen(&mut maps);
        maps.leave(None);
        assert!(maps.anything_unsaved());
    }

    // A name that matches nothing must leave the map on screen where it is.
    #[test]
    fn switching_to_an_unknown_name_changes_nothing() {
        let mut maps = CombatMaps::default();
        maps.open(painted("Ford"), "ford.ron".to_owned(), bookmark(1.0));
        assert!(maps.switch_to("Nowhere", bookmark(9.0)).is_err());
        assert_eq!(maps.on_screen().unwrap().content().name(), "Ford");
        assert_eq!(maps.leave(None), Some(bookmark(1.0)));
    }

    // An open map is reached through its own row, so the stored list leaves out every file
    // an open map is saved as, and a file is found among the open maps by that name.
    #[test]
    fn a_stored_file_an_open_map_holds_is_not_listed_as_stored() {
        let mut maps = CombatMaps::default();
        assert!(maps.needs_listing());
        maps.open(painted("Ford"), "ford.ron".to_owned(), None);
        maps.set_stored(vec!["bridge.ron".to_owned(), "ford.ron".to_owned()]);
        assert!(!maps.needs_listing());
        assert_eq!(maps.stored_not_open(), ["bridge.ron"]);
        assert_eq!(maps.switch_to_file("ford.ron", None), Ok(None));
        assert!(maps.switch_to_file("bridge.ron", None).is_err());
    }

    // *Save and close* writes a map nobody is looking at, and a written map is listed again
    // so it appears among the stored ones.
    #[test]
    fn saving_everything_writes_a_parked_map_and_leaves_it_clean() {
        let root = tempfile::tempdir().unwrap();
        let mut maps = CombatMaps::default();
        maps.open(painted("Ford"), "ford.ron".to_owned(), None);
        paint_on_screen(&mut maps);
        maps.leave(None);
        maps.set_stored(Vec::new());

        maps.save_everything(root.path()).unwrap();
        assert!(!maps.anything_unsaved());
        assert!(maps.needs_listing());
        let loaded = CombatMap::load(&layout::combat_map(root.path(), "ford.ron"), "ford").unwrap();
        assert_eq!(loaded.grid().get(2, 2), Some(CombatTile::Mud));
        assert_eq!(loaded.grid().get(1, 1), Some(CombatTile::Tree));
    }

    // The issue's fourth acceptance step: checking the world map mid-fight and coming back
    // keeps every token where it was.
    #[test]
    fn tokens_survive_parking_the_map() {
        let mut maps = CombatMaps::default();
        maps.open(painted("Ford"), "ford.ron".to_owned(), None);
        let (tokens, extent) = maps.tokens_on_screen_mut().unwrap();
        tokens.place("orc", 2, (3, 4), extent).unwrap();

        maps.leave(None);
        assert!(maps.tokens_on_screen().is_none());
        maps.switch_to("Ford", None).unwrap();

        let (tokens, extent) = maps.tokens_on_screen().unwrap();
        assert_eq!(extent, (8, 8));
        let token = tokens.get("orc1").unwrap();
        assert_eq!((token.x(), token.y(), token.size()), (3, 4, 2));
    }

    // The issue's fifth acceptance step: a saved map carries no token, so `git diff` in the
    // campaign shows none.
    #[test]
    fn a_saved_map_carries_no_token() {
        let root = tempfile::tempdir().unwrap();
        let mut maps = CombatMaps::default();
        maps.open(painted("Ford"), "ford.ron".to_owned(), None);
        paint_on_screen(&mut maps);
        let (tokens, extent) = maps.tokens_on_screen_mut().unwrap();
        tokens.place("goblinchief", 1, (0, 0), extent).unwrap();

        maps.save_on_screen(root.path()).unwrap().unwrap();
        let text = std::fs::read_to_string(layout::combat_map(root.path(), "ford.ron")).unwrap();
        assert!(!text.contains("goblinchief"), "{text}");
        assert_eq!(maps.tokens_on_screen().unwrap().0.len(), 1, "saving keeps the board");
    }
}
