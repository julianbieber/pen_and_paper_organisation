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
/// The vocabulary of change, and every reason one is refused.
pub mod edit;
/// What the GM draws: a feature's identity, shape and kind.
pub mod feature;
/// The names the campaign format fixes, and the joins onto a campaign root.
pub mod layout;
/// What `campaign.ron` says, and reading it back off disk.
pub mod manifest;
/// Which tile a terrain cell is drawn as, and how brightly it is lit.
pub mod tiles;
/// The set of features a map holds, and every rule a set of them must satisfy.
pub mod world;

pub use atlas::{AtlasError, AtlasMeta};
pub use campaign::{Campaign, CampaignError};
pub use document::{Document, UNDO_LIMIT};
pub use edit::{Edit, EditError};
pub use feature::{CellPoint, Feature, FeatureId, FeatureKind, Geometry};
pub use manifest::{
    CAMPAIGN_VERSION, CampaignManifest, DEFAULT_UNIT, DEFAULT_UNITS_PER_CELL, MAX_MANIFEST_BYTES,
};
pub use world::{MAX_WORLD_BYTES, ParentProblem, WORLD_VERSION, WorldError};
