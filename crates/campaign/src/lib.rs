//! Everything a campaign is made of that does not need to be looked at: what is on
//! disk, what it means, and the rules deciding whether it is valid.
//!
//! Nothing here needs a window or a GPU, which is the point of the crate — a rule
//! that can only be checked by looking at it is a rule that will not be checked. The
//! editor depends on this crate; this crate knows nothing about the editor.

/// The tileset sidecar `bevy_sprite_editor` writes, and why one is unusable.
pub mod atlas;
/// The ways a stroke turns a grid and a gesture into the cells it would change.
pub mod brush;
/// Choosing a directory by walking the tree, and why one could not be read.
pub mod browse;
/// Opening and creating a campaign directory, and why either failed.
pub mod campaign;
/// A combat map: a grid of combat tiles and nothing else, and every reason one is refused.
pub mod combat;
/// A value under authorship: its undo and redo stacks, and whether it is unsaved.
pub mod document;
/// A shape part-way through being drawn, and the feature it becomes.
pub mod draft;
/// The vocabulary of change, and every reason one is refused.
pub mod edit;
/// What the GM draws: a feature's identity, shape and kind.
pub mod feature;
/// The square tile grid a document may be drawn on, and every reason one is refused.
pub mod grid;
/// The image a document may be drawn over, and getting the file into the campaign.
pub mod image;
/// The single Edit one authoring gesture becomes.
pub mod gesture;
/// Where a label sits, what it outranks, and which labels fit without overlapping.
pub mod label;
/// The names the campaign format fixes, and the joins onto a campaign root.
pub mod layout;
/// How much detail a feature is drawn at, for a given map scale.
pub mod lod;
/// What `campaign.ron` says, reading it back off disk, and changing the scale in it.
pub mod manifest;
/// What one cell of a document is worth, and every figure derived from it.
pub mod measure;
/// What makes the notes directory a zk notebook, what a note is found by, and the one
/// place this workspace runs zk.
pub mod notebook;
/// What a position in cells lands on, what a rectangle encloses, and where a vertex snaps.
pub mod pick;
/// The campaigns opened before, where that list is kept, and whether one of them is
/// still there.
pub mod recent;
/// What makes a campaign directory a git repository, and the one place this workspace
/// runs git — including syncing one with its remote and cloning one from a remote.
pub mod repo;
/// Turning a label the GM typed into the name of a file this tool creates.
pub mod slug;
/// The one table saying what a feature and a dungeon tile look like.
pub mod style;
/// Which tile a cell is drawn as, and how brightly it is lit.
pub mod tiles;
/// The window title, and the one thing it is allowed to say.
pub mod title;
/// What a map document holds, and every rule such a document must satisfy.
pub mod world;

pub use atlas::{AtlasError, AtlasMeta};
pub use browse::{BrowseError, Listing, Subdirectory};
pub use brush::{Brush, TileChange};
pub use campaign::{Campaign, CampaignError, Created};
pub use document::{Authored, Document, UNDO_LIMIT};
pub use draft::{Draft, DraftShape};
pub use edit::{Edit, EditError};
pub use feature::{CellPoint, Feature, FeatureId, FeatureKind, Geometry, Rank};
pub use gesture::Orphans;
pub use grid::{DEFAULT_GRID_CELLS, DEFAULT_METRES_PER_CELL, GridProblem, MAX_GRID_CELLS, TileGrid, TileVocabulary};
pub use image::{ImageBackdrop, ImageProblem, Imported, ImportError};
pub use tiles::{CombatTile, DungeonTile};
pub use combat::{CombatEdit, CombatMap, CombatProblem, DEFAULT_COMBAT_CELLS};
pub use pick::{Hit, Landing, Snap};
pub use recent::{Listed, MAX_RECENT, Recent, RecentError, Recents};
pub use slug::dungeon_name;
pub use notebook::{NewNote, NoteError, NoteKind, Notebook, Runner, SystemRunner};
pub use repo::{GitError, GitRunner, RemoteOutcome, Repo, SystemGit, Synced};
pub use manifest::{
    CAMPAIGN_VERSION, CampaignManifest, DEFAULT_UNIT, DEFAULT_UNITS_PER_CELL, MAX_MANIFEST_BYTES,
    MAX_UNITS_PER_CELL, MIN_UNITS_PER_CELL,
};
pub use measure::{CellWorth, DistanceUnit, Leg, Measurement, UnitFamily};
pub use world::{MAX_WORLD_BYTES, ParentProblem, WORLD_VERSION, World, WorldError};
