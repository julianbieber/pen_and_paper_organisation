//! The one conversion between terrain cells and world units, and the depth the
//! terrain draws at.
//!
//! A terrain's rows run top to bottom and the world's y runs up, so something has to
//! turn one into the other. Everything that needs to — placing a chunk, clamping the
//! camera, turning a cursor back into a cell — goes through here, because the
//! conversion is the kind of thing that is wrong in two places the moment it is
//! written twice.

use bevy::prelude::*;
use campaign::tiles::CHUNK_CELLS;

/// The depth the terrain is drawn at.
///
/// It is the backdrop, so it is the floor: everything authored on top of it takes a
/// greater z.
pub const TERRAIN_Z: f32 = 0.0;

/// The depth an imported image backdrop is drawn at, between the two.
///
/// Greater than [`TERRAIN_Z`] is what puts the picture over the chunks: a sprite sorts
/// against a tilemap by its z, so this number is the whole of that ordering and a picture
/// at or below the terrain's depth would tie with it and land arbitrarily.
///
/// Being less than [`FEATURE_Z`] is **not** what puts it under the features, and a reader
/// changing this must know that. A feature is a gizmo, every gizmo is queued at one depth
/// with the comparison always passing, and the gizmo pass runs last — so a feature covers
/// this picture at any z whatsoever. The dungeon's grid lines are gizmos too, which is why
/// they are drawn over a scan rather than under it, and that is what registering a scan
/// against a grid wants.
pub const IMAGE_Z: f32 = 0.5;

/// The depth authored features are drawn at, above the terrain.
pub const FEATURE_Z: f32 = 1.0;

const _: () = {
    assert!(TERRAIN_Z < IMAGE_Z);
    assert!(IMAGE_Z < FEATURE_Z);
};

/// How a terrain's cells sit in the world.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MapView {
    /// World units one terrain cell spans, which is also a tile's size in pixels, so
    /// the art is drawn at its own scale when the camera is not zoomed.
    pub cell_size: f32,
    pub width: u32,
    pub height: u32,
}

impl MapView {
    /// A terrain `width` by `height` cells, drawn `cell_size` world units to the cell.
    ///
    /// `cell_size` is the tileset's own tile size in pixels, so a cell is one tile and
    /// the art is drawn at the size it was painted when the camera is not zoomed.
    pub fn new(width: u32, height: u32, cell_size: f32) -> Self {
        Self {
            cell_size,
            width,
            height,
        }
    }

    /// Where the middle of a terrain cell sits in the world.
    ///
    /// Row zero comes back at the top: this negation is the flip, and it is the only
    /// one outside the tile order inside a chunk.
    pub fn cell_to_world(self, x: f32, y: f32) -> Vec2 {
        Vec2::new((x + 0.5) * self.cell_size, -(y + 0.5) * self.cell_size)
    }

    /// Where the top-left corner of a cell sits in the world, rather than its middle.
    ///
    /// [`Self::cell_to_world`] answers with the middle, which is what a tile and a label
    /// want and what a rectangle anchored on a corner does not. Written here because the
    /// half-cell correction is the same kind of thing as the flip: correct in one place, or
    /// wrong in every place it is repeated.
    pub fn cell_corner_to_world(self, x: f32, y: f32) -> Vec2 {
        self.cell_to_world(x, y) + Vec2::new(-self.cell_size / 2.0, self.cell_size / 2.0)
    }

    /// Which terrain cell a world point falls on. Fractional, and may be outside the
    /// terrain — the caller decides what that means.
    pub fn world_to_cell(self, point: Vec2) -> Vec2 {
        Vec2::new(
            point.x / self.cell_size - 0.5,
            -point.y / self.cell_size - 0.5,
        )
    }

    /// World units a chunk spans on a side.
    pub fn chunk_size(self) -> f32 {
        self.cell_size * CHUNK_CELLS as f32
    }

    /// Where a chunk's entity sits, so that its own tile order lands each cell exactly
    /// where [`Self::cell_to_world`] puts it.
    ///
    /// A chunk is centred on the middle of the cells it covers, and that middle goes
    /// through the same conversion as any other cell — the flip is written once or it
    /// is written wrong.
    pub fn chunk_translation(self, chunk_x: i32, chunk_y: i32) -> Vec3 {
        let half = CHUNK_CELLS as f32 / 2.0 - 0.5;
        let centre = self.cell_to_world(
            chunk_x as f32 * CHUNK_CELLS as f32 + half,
            chunk_y as f32 * CHUNK_CELLS as f32 + half,
        );
        centre.extend(TERRAIN_Z)
    }

    /// The whole terrain, in world units.
    pub fn extent(self) -> Rect {
        Rect::new(
            0.0,
            -(self.height as f32) * self.cell_size,
            self.width as f32 * self.cell_size,
            0.0,
        )
    }

    /// The terrain grown by a margin of `cells`, which is what the camera is held
    /// inside so the map does not float away from the view.
    pub fn extent_with_margin(self, cells: f32) -> Rect {
        self.extent().inflate(cells * self.cell_size)
    }

    /// Which chunks a world rectangle touches, as inclusive `x` then `y` ranges.
    ///
    /// Divides towards negative infinity: a camera panned past the origin sees
    /// negative chunk coordinates, and truncating would fold two chunk rows into one.
    pub fn chunks_over(self, rect: Rect) -> (std::ops::RangeInclusive<i32>, std::ops::RangeInclusive<i32>) {
        let side = self.chunk_size();
        let chunk_of = |value: f32| (value / side).floor() as i32;
        (
            chunk_of(rect.min.x)..=chunk_of(rect.max.x),
            chunk_of(-rect.max.y)..=chunk_of(-rect.min.y),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // The flip has to survive being written once: a chunk's own placement and a cell's
    // must agree, or north is right inside each chunk and wrong between them.
    #[test]
    fn a_chunks_placement_agrees_with_the_cells_it_covers() {
        let view = MapView::new(4096, 4096, 16.0);
        let side = view.chunk_size();

        for (cx, cy) in [(0, 0), (1, 0), (0, 1), (-1, -1), (3, -2)] {
            let translation = view.chunk_translation(cx, cy);
            let first = view.cell_to_world(
                cx as f32 * CHUNK_CELLS as f32,
                cy as f32 * CHUNK_CELLS as f32,
            );
            assert!(
                (translation.x - (first.x - view.cell_size / 2.0 + side / 2.0)).abs() < 0.01,
                "chunk {cx},{cy} sits at {translation:?}"
            );
            assert!(
                (translation.y - (first.y + view.cell_size / 2.0 - side / 2.0)).abs() < 0.01,
                "chunk {cx},{cy} sits at {translation:?}"
            );
        }
    }

    // North is up: the terrain's first row must come back above its second.
    #[test]
    fn the_terrains_rows_run_downwards_in_the_world() {
        let view = MapView::new(8, 8, 16.0);
        assert!(view.cell_to_world(0.0, 0.0).y > view.cell_to_world(0.0, 1.0).y);
        assert!(view.chunk_translation(0, 0).y > view.chunk_translation(0, 1).y);
    }

    // A camera panned past the origin sees negative chunk coordinates, and truncating
    // division would fold chunks -1 and 0 into one and drop a row at every boundary.
    #[test]
    fn chunks_over_a_rectangle_divides_towards_negative_infinity() {
        let view = MapView::new(4096, 4096, 16.0);
        let side = view.chunk_size();
        let rect = Rect::new(-side * 0.5, -side * 1.5, side * 0.5, side * 0.5);
        let (columns, rows) = view.chunks_over(rect);
        assert_eq!(*columns.start(), -1);
        assert_eq!(*columns.end(), 0);
        assert_eq!(*rows.start(), -1);
        assert_eq!(*rows.end(), 1);
    }

    // A corner is half a cell up and left of the middle, and the flip means "up" is +y in
    // the world while it is -y in cells. Getting that sign wrong puts an anchored picture
    // one cell out in each axis, which reads as a placement bug rather than a conversion.
    #[test]
    fn a_cells_corner_sits_above_and_left_of_its_middle() {
        let view = MapView::new(64, 64, 16.0);
        let middle = view.cell_to_world(4.0, 7.0);
        let corner = view.cell_corner_to_world(4.0, 7.0);

        assert!((corner.x - (middle.x - 8.0)).abs() < 0.001, "{corner:?}");
        assert!((corner.y - (middle.y + 8.0)).abs() < 0.001, "{corner:?}");
    }

    // World and cell must round-trip, or a cursor cannot be turned back into a cell.
    #[test]
    fn a_cell_survives_the_trip_through_the_world() {
        let view = MapView::new(100, 100, 16.0);
        for (x, y) in [(0.0, 0.0), (7.0, 3.0), (99.0, 99.0)] {
            let back = view.world_to_cell(view.cell_to_world(x, y));
            assert!((back.x - x).abs() < 0.001 && (back.y - y).abs() < 0.001, "{back:?}");
        }
    }
}
