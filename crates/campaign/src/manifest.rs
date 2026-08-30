//! What `campaign.ron` says, and how it is read back from a directory that may not
//! have been written by this build.
//!
//! The manifest is the serialized form and nothing more — it names where a terrain
//! is, it never holds one. Reading it is separate from opening a campaign because
//! every way a campaign directory can be wrong *before* its terrain is reached is
//! decided here, from a few hundred bytes, and a caller may want that answer without
//! paying for a terrain load.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::campaign::CampaignError;
use crate::layout;

/// The campaign format this build writes, and the only one it reads.
///
/// There is no migration path: a directory carrying any other version is refused
/// rather than upgraded.
pub const CAMPAIGN_VERSION: u32 = 1;

/// The largest `campaign.ron` this build will read, checked before the file is read
/// rather than after.
pub const MAX_MANIFEST_BYTES: u64 = 1024 * 1024;

/// The scale a campaign gets when it is created, in `DEFAULT_UNIT` per terrain cell.
pub const DEFAULT_UNITS_PER_CELL: f64 = 1.0;

/// The distance unit a campaign gets when it is created.
pub const DEFAULT_UNIT: &str = "km";

/// What `campaign.ron` holds.
///
/// Read back with [`CampaignManifest::read`], which is the only thing that checks
/// any of these values. Constructing one directly asserts nothing.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CampaignManifest {
    /// The format version, checked against [`CAMPAIGN_VERSION`] before any other
    /// field of the file is understood.
    pub version: u32,
    /// What the GM calls this campaign. Display only.
    pub name: String,
    /// Where the terrain is, exactly as it was given: resolved against the campaign
    /// root when relative, taken as written when absolute. Never empty.
    pub terrain: String,
    /// How many `unit`s one terrain cell spans. Finite and above zero.
    pub units_per_cell: f64,
    /// The distance unit `units_per_cell` counts, as the GM reads it — "km", "miles",
    /// "leagues", "days". A display label: nothing branches on its value.
    pub unit: String,
}

#[derive(Deserialize)]
#[serde(rename = "CampaignManifest")]
struct ManifestVersion {
    version: u32,
}

impl CampaignManifest {
    /// The manifest in the campaign directory at `root`.
    ///
    /// Fails [`CampaignError::NotACampaign`] when `root` holds no `campaign.ron` or
    /// what it holds is not a regular file; [`CampaignError::ManifestUnreadable`]
    /// when the file is over [`MAX_MANIFEST_BYTES`] or cannot be read;
    /// [`CampaignError::UnsupportedVersion`] when it declares a version other than
    /// [`CAMPAIGN_VERSION`]; and [`CampaignError::ManifestMalformed`] when it is not
    /// valid RON for this type, or carries a `units_per_cell` that is not finite and
    /// above zero, or an empty `terrain`.
    ///
    /// The version is checked before the rest of the file is deserialized, so a
    /// later format that renames or drops a field is still refused for its version
    /// rather than for being unparseable. Fields this build does not know are
    /// ignored, so a later additive field stays readable here.
    pub fn read(root: &Path) -> Result<Self, CampaignError> {
        let path = layout::manifest(root);

        match std::fs::symlink_metadata(&path) {
            Ok(meta) if !meta.is_file() => return Err(CampaignError::NotACampaign(root.to_owned())),
            Err(_) => return Err(CampaignError::NotACampaign(root.to_owned())),
            Ok(_) => {}
        }

        let len = std::fs::metadata(&path)
            .map_err(|source| CampaignError::ManifestUnreadable {
                path: path.clone(),
                source,
            })?
            .len();
        if len > MAX_MANIFEST_BYTES {
            return Err(CampaignError::ManifestUnreadable {
                path: path.clone(),
                source: std::io::Error::new(
                    std::io::ErrorKind::FileTooLarge,
                    format!("{len} bytes, over the {MAX_MANIFEST_BYTES} byte limit"),
                ),
            });
        }

        let text =
            std::fs::read_to_string(&path).map_err(|source| CampaignError::ManifestUnreadable {
                path: path.clone(),
                source,
            })?;

        let probe: ManifestVersion = ron::from_str(&text)
            .map_err(|error| CampaignError::ManifestMalformed(error.to_string()))?;
        if probe.version != CAMPAIGN_VERSION {
            return Err(CampaignError::UnsupportedVersion {
                found: probe.version,
                expected: CAMPAIGN_VERSION,
            });
        }

        let manifest: Self = ron::from_str(&text)
            .map_err(|error| CampaignError::ManifestMalformed(error.to_string()))?;
        manifest.check()?;
        Ok(manifest)
    }

    /// The directory `terrain` names, resolved against the campaign root.
    ///
    /// A relative `terrain` resolves under `root`; an absolute one is taken as
    /// written. That is [`Path::join`]'s own rule, and it is relied on here rather
    /// than reimplemented — a terrain is allowed to live outside the campaign
    /// directory, because the manifest records where one is rather than owning it.
    pub fn terrain_dir(&self, root: &Path) -> PathBuf {
        root.join(&self.terrain)
    }

    fn check(&self) -> Result<(), CampaignError> {
        if self.terrain.is_empty() {
            return Err(CampaignError::ManifestMalformed(
                "terrain names no directory".to_owned(),
            ));
        }
        if !self.units_per_cell.is_finite() || self.units_per_cell <= 0.0 {
            return Err(CampaignError::ManifestMalformed(format!(
                "units_per_cell is {}, which is not a positive finite scale",
                self.units_per_cell
            )));
        }
        Ok(())
    }
}
