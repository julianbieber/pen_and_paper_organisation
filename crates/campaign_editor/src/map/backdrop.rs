//! What the open document is drawn on, and where the camera should be looking at it.
//!
//! One resource, and it is the only source of a [`MapView`] in the editor. Before it there
//! were seven places that built one out of the terrain's extent, which was fine while the
//! terrain was the only backdrop and became seven chances to measure a dungeon against a
//! terrain the moment it was not. A consumer that wants to know how big a cell is, or how
//! far the map goes, asks here.
//!
//! It also carries where the camera goes, because the two questions are the same question:
//! a backdrop that has just been swapped in either has a remembered camera to restore or
//! needs framing, and nothing else may decide which. That is why the restore lives here
//! rather than being written onto the camera by whoever switched — a system in the
//! authoring set writing the transform would be undone by the framing system in the map
//! set on the following frame.

use bevy::prelude::*;

use crate::map::view::MapView;

/// Which backdrop the open document is drawn on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackdropSource {
    /// The campaign's imported terrain, which the world map is drawn on.
    Terrain,
    /// The open document's own tile grid, which a dungeon is drawn on.
    Grid,
}

/// Where the camera was looking at a backdrop, so that leaving one and coming back does
/// not lose the place.
///
/// Session state and deliberately not part of any document: where a GM last happened to be
/// looking is not something a campaign directory should carry between machines.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CameraBookmark {
    pub translation: Vec2,
    pub scale: f32,
}

/// What the open document is drawn on, how big it is, and where to look at it.
///
/// `generation` rises on every switch and is what tells the framing system that this is a
/// backdrop it has not placed the camera for yet. A change-detection flag would not do:
/// framing legitimately declines on a frame where the window has no viewport, and
/// `is_changed` is true for one frame only, so the retry would be lost.
#[derive(Resource, Debug, Clone, Copy)]
pub struct Backdrop {
    /// How the backdrop's cells sit in the world — the one [`MapView`] in the editor.
    pub view: MapView,
    pub source: BackdropSource,
    /// Rises on every switch, so the camera is placed once per backdrop rather than once
    /// per process.
    pub generation: u32,
    /// Where the camera should go, or `None` to fit the whole backdrop in view.
    pub restore: Option<CameraBookmark>,
}

impl Backdrop {
    /// The backdrop a terrain `width` by `height` cells makes, at `cell_size` world units
    /// to the cell.
    pub fn terrain(width: u32, height: u32, cell_size: f32) -> Self {
        Self {
            view: MapView::new(width, height, cell_size),
            source: BackdropSource::Terrain,
            generation: 0,
            restore: None,
        }
    }

    /// Whether this backdrop is the open document's own grid.
    pub fn is_grid(&self) -> bool {
        self.source == BackdropSource::Grid
    }

    /// Become `next`, one generation on, to be looked at from `restore` or framed afresh.
    ///
    /// The generation rises here and nowhere else, which is what keeps "has the camera been
    /// placed for this backdrop" answerable by comparing two numbers.
    pub fn switch_to(&mut self, view: MapView, source: BackdropSource, restore: Option<CameraBookmark>) {
        self.view = view;
        self.source = source;
        self.generation = self.generation.wrapping_add(1);
        self.restore = restore;
    }
}

/// Whether the live backdrop is the campaign's terrain.
pub fn backdrop_is_the_terrain(backdrop: Option<Res<Backdrop>>) -> bool {
    backdrop.is_some_and(|backdrop| !backdrop.is_grid())
}
