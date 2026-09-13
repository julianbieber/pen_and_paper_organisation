//! A combat map as its own kind of document — a grid of combat tiles and nothing else —
//! the one change it is made by, and every reason one is refused.
//!
//! It carries no features, no notes and no picture, so it shares the grid, the brushes
//! and the paint rule with a dungeon and nothing else a [`World`](crate::world::World)
//! holds.

use serde::{Deserialize, Serialize};

use crate::brush::TileChange;
use crate::document::Authored;
use crate::edit::EditError;
use crate::grid::{DEFAULT_METRES_PER_CELL, GridProblem, TileGrid};
use crate::slug::slug_of;
use crate::tiles::CombatTile;

/// Cells along each side of a combat map the form opens when no size is typed.
pub const DEFAULT_COMBAT_CELLS: u32 = 30;

/// A combat map that cannot be made.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum CombatProblem {
    /// A name nothing survives [`slug_of`] in: empty, blank, or punctuation only.
    #[error("a combat map needs a name with a letter or a digit in it")]
    Unnamed,

    /// A grid the map could not hold.
    #[error(transparent)]
    Grid(#[from] GridProblem),
}

/// A named grid of combat tiles, measured five feet to the cell.
///
/// Every field is private and [`CombatEdit`] is the only way to change a tile, so the
/// panel and the control socket cannot come to mean different things. A combat map that
/// exists always has a name with a letter or a digit in it and a coherent grid.
#[derive(Debug, Clone, PartialEq)]
pub struct CombatMap {
    name: String,
    grid: TileGrid<CombatTile>,
}

impl CombatMap {
    /// A combat map called `name`, `width` by `height` cells, every cell
    /// [`CombatTile::Grass`], each cell [`DEFAULT_METRES_PER_CELL`] across.
    ///
    /// The stored name is `name` with the surrounding whitespace trimmed. Fails
    /// [`CombatProblem::Unnamed`] when the name yields no slug, which is checked before the
    /// size, and [`CombatProblem::Grid`] with [`GridProblem::NoExtent`] or
    /// [`GridProblem::TooManyCells`] when the size is not one a grid may have.
    pub fn new(name: &str, width: u32, height: u32) -> Result<Self, CombatProblem> {
        if slug_of(name).is_none() {
            return Err(CombatProblem::Unnamed);
        }
        let grid = TileGrid::new(width, height, DEFAULT_METRES_PER_CELL)?;
        Ok(Self {
            name: name.trim().to_owned(),
            grid,
        })
    }

    /// What the GM called it.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The tiles it is painted with.
    pub fn grid(&self) -> &TileGrid<CombatTile> {
        &self.grid
    }
}

/// A change to a combat map.
///
/// Deliberately not `#[non_exhaustive]`, for the reason [`Edit`](crate::edit::Edit) is
/// not: whatever applies or parses one should fail to compile when a variant is added.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum CombatEdit {
    /// One brush stroke: every cell it changes, each with the tile it becomes.
    PaintTiles { changes: Vec<TileChange<CombatTile>> },
}

impl CombatEdit {
    /// Apply this edit to `map` and hand back the edit that undoes it.
    ///
    /// Fails [`EditError::CellOutOfGrid`] when a change names a cell outside the map and
    /// [`EditError::EmptyPaint`] when no change would alter its cell — the refusals a
    /// dungeon stroke gets. A refused edit leaves the map untouched.
    pub fn apply(self, map: &mut CombatMap) -> Result<Self, EditError> {
        match self {
            Self::PaintTiles { changes } => crate::edit::paint(&mut map.grid, changes)
                .map(|changes| Self::PaintTiles { changes }),
        }
    }
}

impl Authored for CombatMap {
    type Edit = CombatEdit;
    type Refusal = EditError;

    fn apply_edit(&mut self, edit: CombatEdit) -> Result<CombatEdit, EditError> {
        edit.apply(self)
    }
}

/// `wanted`, trimmed, when no name in `taken` is the same; otherwise the first of
/// `"{wanted} 2"`, `"{wanted} 3"`, … that is free.
pub fn unused_name<'a>(wanted: &str, taken: impl IntoIterator<Item = &'a str>) -> String {
    let wanted = wanted.trim();
    let taken: Vec<&str> = taken.into_iter().collect();
    if !taken.contains(&wanted) {
        return wanted.to_owned();
    }
    for suffix in 2u32.. {
        let candidate = format!("{wanted} {suffix}");
        if !taken.contains(&candidate.as_str()) {
            return candidate;
        }
    }
    unreachable!("the suffix range is unbounded, so some candidate is always free")
}
