//! The combat maps open this session, and which one, if any, is on screen.
//!
//! It sits at the editor's root beside `document.rs`, for that module's reason: the map
//! draws the grid of the combat map on screen and authoring paints it.
//!
//! A combat map sits over whatever [`WorldDoc`](crate::document::WorldDoc) has on screen
//! and leaves it untouched, so going back to the map returns to exactly that document. A
//! combat map that is not on screen is parked with its undo stack, its dirty flag and
//! where the camera was looking at it, as a parked world document is.

use bevy::prelude::*;
use campaign::{CombatMap, Document};

use crate::map::backdrop::CameraBookmark;

/// The most combat maps open at once — as many as the panel has rows for.
pub const MAX_OPEN_COMBAT_MAPS: usize = 8;

/// A combat map that is open this session, and where the camera was left on it.
#[derive(Debug)]
pub struct OpenCombatMap {
    pub document: Document<CombatMap>,
    /// Where the camera was looking at it when it was parked, or `None` if it never was.
    pub camera: Option<CameraBookmark>,
}

/// Every combat map open this session, in the order they were opened, and which one is on
/// screen.
///
/// Session state: nothing here is ever written, and [`crate::session::close_campaign`]
/// resets it.
#[derive(Resource, Debug, Default)]
pub struct CombatMaps {
    maps: Vec<OpenCombatMap>,
    on_screen: Option<usize>,
    under: Option<CameraBookmark>,
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
    /// returns to. The caller checks [`CombatMaps::is_full`] first; this does not refuse.
    pub fn open(&mut self, map: CombatMap, here: Option<CameraBookmark>) {
        self.park(here);
        self.maps.push(OpenCombatMap {
            document: Document::new(map),
            camera: None,
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
        self.park(here);
        self.on_screen = Some(index);
        Ok(self.maps[index].camera)
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
        maps.open(painted("Ford"), bookmark(1.0));
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
        maps.open(painted("Ford"), bookmark(1.0));
        maps.open(painted("Bridge"), bookmark(5.0));
        assert_eq!(maps.leave(bookmark(6.0)), Some(bookmark(1.0)));
        assert_eq!(maps.leave(bookmark(7.0)), None);
    }

    // The close guard asks about a map nobody is looking at: a parked, painted map is lost
    // just as surely on quit as the one on screen.
    #[test]
    fn a_parked_dirty_map_counts_as_unsaved() {
        let mut maps = CombatMaps::default();
        assert!(!maps.anything_unsaved());
        maps.open(painted("Ford"), None);
        assert!(!maps.anything_unsaved(), "a new map is clean");
        paint_on_screen(&mut maps);
        maps.leave(None);
        assert!(maps.anything_unsaved());
    }

    // A name that matches nothing must leave the map on screen where it is.
    #[test]
    fn switching_to_an_unknown_name_changes_nothing() {
        let mut maps = CombatMaps::default();
        maps.open(painted("Ford"), bookmark(1.0));
        assert!(maps.switch_to("Nowhere", bookmark(9.0)).is_err());
        assert_eq!(maps.on_screen().unwrap().content().name(), "Ford");
        assert_eq!(maps.leave(None), Some(bookmark(1.0)));
    }
}
