//! A combat map as its own kind of document — a grid of combat tiles and nothing else —
//! the one change it is made by, and every reason one is refused.
//!
//! It carries no features, no notes and no picture, so it shares the grid, the brushes
//! and the paint rule with a dungeon and nothing else a [`World`](crate::world::World)
//! holds. Where one is stored, it is stored as its grid alone, and it is refused on the
//! way back in for the reasons a dungeon's grid is.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::brush::TileChange;
use crate::document::Authored;
use crate::edit::EditError;
use crate::feature::{DUNGEON_EXTENSION, dungeon_name_refusal};
use crate::grid::{DEFAULT_METRES_PER_CELL, GridProblem, TileGrid};
use crate::layout;
use crate::slug::slug_of;
use crate::tiles::CombatTile;
use crate::world::{WorldError, read_document, write_document};

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

    /// This map as the RON text [`CombatMap::save`] writes: the grid, run-length encoded,
    /// and nothing else — not the name, and not a version.
    pub fn to_ron(&self) -> Result<String, ron::Error> {
        ron::ser::to_string_pretty(&self.grid, ron::ser::PrettyConfig::default())
    }

    /// The combat map this RON text describes, named `name` with its surrounding
    /// whitespace trimmed.
    ///
    /// `path` appears in the error messages and is not read. Fails
    /// [`WorldError::WorldMalformed`] for anything RON refuses and for a `name` with no
    /// letter or digit in it, and [`WorldError::BadGrid`] for a grid a document may not
    /// hold — the variant a dungeon's grid is refused with, so the sentence is the same.
    pub fn from_ron(text: &str, path: &Path, name: &str) -> Result<Self, WorldError> {
        let grid: TileGrid<CombatTile> =
            ron::from_str(text).map_err(|error| WorldError::WorldMalformed {
                path: path.to_owned(),
                reason: error.to_string(),
            })?;
        if let Some(source) = grid.refusal() {
            return Err(WorldError::BadGrid {
                path: path.to_owned(),
                source,
            });
        }
        if slug_of(name).is_none() {
            return Err(WorldError::WorldMalformed {
                path: path.to_owned(),
                reason: "its name has no letter or digit to call the map by".to_owned(),
            });
        }
        Ok(Self {
            name: name.trim().to_owned(),
            grid,
        })
    }

    /// The combat map stored at `path`, named `name`.
    ///
    /// Unlike a world document, an absent file is not an empty map: it fails
    /// [`WorldError::WorldUnreadable`], as does something at `path` that is not a regular
    /// file or is too large to read. Otherwise fails as [`CombatMap::from_ron`] does.
    /// Writes nothing.
    pub fn load(path: &Path, name: &str) -> Result<Self, WorldError> {
        match read_document(path)? {
            Some(text) => Self::from_ron(&text, path, name),
            None => Err(WorldError::WorldUnreadable {
                path: path.to_owned(),
                source: std::io::Error::from(std::io::ErrorKind::NotFound),
            }),
        }
    }

    /// Write this map to `path`, replacing whatever is there.
    ///
    /// Written as [`World::save`](crate::world::World::save) writes a world: through a
    /// sibling temporary renamed over the target, creating `path`'s parent first, so a
    /// campaign that has never stored a combat map can store one. Fails
    /// [`WorldError::WorldTooLarge`] or [`WorldError::WorldUnwritable`] naming `path`.
    pub fn save(&self, path: &Path) -> Result<(), WorldError> {
        let text = self.to_ron().map_err(|error| WorldError::WorldUnwritable {
            path: path.to_owned(),
            source: std::io::Error::other(error),
        })?;
        write_document(path, &text)
    }
}

/// Why `name` is not the file name of a stored combat map, or `None` if it is fine.
///
/// The rule a dungeon's file name is held to: one component, no separator, no `..`, and a
/// `.ron` extension.
pub fn file_name_refusal(name: &str) -> Option<&'static str> {
    dungeon_name_refusal(name)
}

/// The file names of every combat map stored under the campaign at `root`, sorted.
///
/// An absent `combat` directory is an empty list. Only regular files are listed, and only
/// those whose name is UTF-8, passes [`file_name_refusal`] — which a save's temporary does
/// not — and has a stem that is not hidden. A listed file is not promised to load.
///
/// Fails with the error reading the directory itself.
pub fn stored(root: &Path) -> std::io::Result<Vec<String>> {
    let entries = match std::fs::read_dir(layout::combat(root)) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error),
    };

    let mut names = Vec::new();
    for entry in entries {
        let entry = entry?;
        if !entry.file_type()?.is_file() {
            continue;
        }
        let Ok(name) = entry.file_name().into_string() else {
            continue;
        };
        let stem = stem_of(&name);
        if file_name_refusal(&name).is_none() && !stem.is_empty() && !stem.starts_with('.') {
            names.push(name);
        }
    }
    names.sort_unstable();
    Ok(names)
}

/// The name a stored file is shown by: `file` without its `.ron`.
pub fn stem_of(file: &str) -> &str {
    file.strip_suffix(DUNGEON_EXTENSION).unwrap_or(file)
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
