//! Everything a campaign is made of that does not need to be looked at: what is on
//! disk, what it means, and the rules deciding whether it is valid.
//!
//! Nothing here needs a window or a GPU, which is the point of the crate — a rule
//! that can only be checked by looking at it is a rule that will not be checked. The
//! editor depends on this crate; this crate knows nothing about the editor.

/// Opening and creating a campaign directory, and why either failed.
pub mod campaign;
/// The names the campaign format fixes, and the joins onto a campaign root.
pub mod layout;
/// What `campaign.ron` says, and reading it back off disk.
pub mod manifest;

pub use campaign::{Campaign, CampaignError};
pub use manifest::{
    CAMPAIGN_VERSION, CampaignManifest, DEFAULT_UNIT, DEFAULT_UNITS_PER_CELL, MAX_MANIFEST_BYTES,
};
