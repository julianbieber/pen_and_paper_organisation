//! Everything a campaign is made of that does not need to be looked at: what is on
//! disk, what it means, and the rules deciding whether it is valid.
//!
//! Nothing here needs a window or a GPU, which is the point of the crate — a rule
//! that can only be checked by looking at it is a rule that will not be checked. The
//! editor depends on this crate; this crate knows nothing about the editor.

/// The tileset sidecar `bevy_sprite_editor` writes, and why one is unusable.
pub mod atlas;
/// Opening and creating a campaign directory, and why either failed.
pub mod campaign;
/// A world under authorship: its undo and redo stacks, and whether it is unsaved.
pub mod document;
/// A shape part-way through being drawn, and the feature it becomes.
pub mod draft;
/// The vocabulary of change, and every reason one is refused.
pub mod edit;
/// What the GM draws: a feature's identity, shape and kind.
pub mod feature;
/// The single Edit one authoring gesture becomes.
pub mod gesture;
/// Where a label sits, what it outranks, and which labels fit without overlapping.
pub mod label;
/// The names the campaign format fixes, and the joins onto a campaign root.
pub mod layout;
/// How much detail a feature is drawn at, for a given map scale.
pub mod lod;
/// What `campaign.ron` says, and reading it back off disk.
pub mod manifest;
/// What makes the notes directory a zk notebook, what a note is found by, and the one
/// place this workspace runs zk.
pub mod notebook;
/// What a position in cells lands on, what a rectangle encloses, and where a vertex snaps.
pub mod pick;
/// The one table saying what a FeatureKind and a Rank look like.
pub mod style;
/// Which tile a terrain cell is drawn as, and how brightly it is lit.
pub mod tiles;
/// The set of features a map holds, and every rule a set of them must satisfy.
pub mod world;

pub use atlas::{AtlasError, AtlasMeta};
pub use campaign::{Campaign, CampaignError};
pub use document::{Document, UNDO_LIMIT};
pub use draft::{Draft, DraftShape};
pub use edit::{Edit, EditError};
pub use feature::{CellPoint, Feature, FeatureId, FeatureKind, Geometry, Rank};
pub use gesture::Orphans;
pub use pick::{Hit, Landing, Snap};
pub use notebook::{NewNote, NoteError, NoteKind, Notebook, Runner, SystemRunner};
pub use manifest::{
    CAMPAIGN_VERSION, CampaignManifest, DEFAULT_UNIT, DEFAULT_UNITS_PER_CELL, MAX_MANIFEST_BYTES,
};
pub use world::{MAX_WORLD_BYTES, ParentProblem, WORLD_VERSION, World, WorldError};
