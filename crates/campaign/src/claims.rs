//! Which terrain cells the features the GM drew claim for a tile of their own.
//!
//! A settlement's polygon draws the cells under it as city and a road's polyline draws
//! the cells it crosses as road, so the world map reads as settled where the GM settled
//! it. The terrain never decides either.

use std::collections::HashMap;

use crate::feature::{CellPoint, FeatureKind, Geometry};
use crate::tiles::{CHUNK_CELLS, ClaimedCell, TileKind};
use crate::world::World;

/// How far apart a road is sampled, in cells; under one half, so no cell a segment
/// crosses is stepped over.
const ROAD_STEP: f32 = 0.4;

/// Every claimed cell of a world map, grouped by the chunk it falls in.
///
/// Two claims compare equal exactly when they would draw the same, which is what says
/// whether an edit needs any chunk redrawn.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Claims {
    chunks: HashMap<(i32, i32), Vec<ClaimedCell>>,
}

impl Claims {
    /// The cells `world`'s settlements and roads claim on a terrain `width` by `height`
    /// cells. A city claims over a road where the two meet. A world carrying a grid is a
    /// dungeon and claims nothing.
    pub fn of(world: &World, width: u32, height: u32) -> Self {
        let mut cells: HashMap<(u32, u32), TileKind> = HashMap::new();
        if world.grid().is_some() {
            return Self::default();
        }
        for (_, feature) in world.features() {
            if let (FeatureKind::Road, Geometry::Polyline(points)) =
                (feature.kind, &feature.geometry)
            {
                for cell in along(points, width, height) {
                    cells.insert(cell, TileKind::Road);
                }
            }
        }
        for (_, feature) in world.features() {
            if let (FeatureKind::Settlement, Geometry::Polygon(points)) =
                (feature.kind, &feature.geometry)
            {
                for cell in inside(points, width, height) {
                    cells.insert(cell, TileKind::City);
                }
            }
        }

        let side = CHUNK_CELLS;
        let mut chunks: HashMap<(i32, i32), Vec<ClaimedCell>> = HashMap::new();
        for ((x, y), kind) in cells {
            let (column, row) = (x % side, y % side);
            chunks
                .entry(((x / side) as i32, (y / side) as i32))
                .or_default()
                .push(ClaimedCell {
                    slot: (side - 1 - row) * side + column,
                    kind,
                });
        }
        for claimed in chunks.values_mut() {
            claimed.sort_by_key(|cell| cell.slot);
        }
        Self { chunks }
    }

    /// The cells claimed in the chunk at `chunk_x, chunk_y`, by tilemap slot.
    pub fn in_chunk(&self, chunk_x: i32, chunk_y: i32) -> &[ClaimedCell] {
        self.chunks
            .get(&(chunk_x, chunk_y))
            .map_or(&[], Vec::as_slice)
    }

    /// The chunks whose claimed cells differ between `self` and `other`.
    pub fn chunks_differing_from(&self, other: &Self) -> Vec<(i32, i32)> {
        let mut differing: Vec<(i32, i32)> = self
            .chunks
            .keys()
            .chain(other.chunks.keys())
            .copied()
            .filter(|&(x, y)| self.in_chunk(x, y) != other.in_chunk(x, y))
            .collect();
        differing.sort_unstable();
        differing.dedup();
        differing
    }
}

fn cell_of(x: f32, y: f32, width: u32, height: u32) -> Option<(u32, u32)> {
    let (x, y) = (x.round(), y.round());
    (x >= 0.0 && y >= 0.0 && x < width as f32 && y < height as f32).then_some((x as u32, y as u32))
}

fn along(points: &[CellPoint], width: u32, height: u32) -> Vec<(u32, u32)> {
    let mut cells = Vec::new();
    for pair in points.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        let length = (b.x - a.x).hypot(b.y - a.y);
        if !length.is_finite() {
            continue;
        }
        let steps = (length / ROAD_STEP).ceil().max(1.0) as u32;
        for step in 0..=steps {
            let t = step as f32 / steps as f32;
            if let Some(cell) = cell_of(a.x + (b.x - a.x) * t, a.y + (b.y - a.y) * t, width, height)
            {
                cells.push(cell);
            }
        }
    }
    cells
}

fn inside(points: &[CellPoint], width: u32, height: u32) -> Vec<(u32, u32)> {
    let finite = points.iter().all(|point| point.is_finite());
    if points.len() < 3 || !finite || width == 0 || height == 0 {
        return Vec::new();
    }
    let low_x = points
        .iter()
        .map(|p| p.x)
        .fold(f32::MAX, f32::min)
        .floor()
        .max(0.0) as u32;
    let low_y = points
        .iter()
        .map(|p| p.y)
        .fold(f32::MAX, f32::min)
        .floor()
        .max(0.0) as u32;
    let high_x = points.iter().map(|p| p.x).fold(f32::MIN, f32::max).ceil();
    let high_y = points.iter().map(|p| p.y).fold(f32::MIN, f32::max).ceil();
    if high_x < 0.0 || high_y < 0.0 {
        return Vec::new();
    }
    let high_x = (high_x as u32).min(width - 1);
    let high_y = (high_y as u32).min(height - 1);

    let mut cells = Vec::new();
    for y in low_y..=high_y {
        for x in low_x..=high_x {
            if contains(points, x as f32, y as f32) {
                cells.push((x, y));
            }
        }
    }
    cells
}

fn contains(points: &[CellPoint], x: f32, y: f32) -> bool {
    let mut odd = false;
    let mut previous = points[points.len() - 1];
    for &point in points {
        if (point.y > y) != (previous.y > y) {
            let crossing =
                point.x + (y - point.y) / (previous.y - point.y) * (previous.x - point.x);
            if x < crossing {
                odd = !odd;
            }
        }
        previous = point;
    }
    odd
}
