//! What `campaign.ron` says, how it is read back from a directory that may not have been
//! written by this build, and how the scale in it is changed.
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

/// The smallest scale `campaign.ron` may declare, in `unit` per cell.
///
/// A floor as well as a ceiling because this number is *divided into* a zoom to size the
/// scale bar. Below this the search for a round figure walks into the subnormals and
/// arrives at zero, which is not a figure any bar can be labelled with.
pub const MIN_UNITS_PER_CELL: f64 = 1.0e-9;

/// The largest scale `campaign.ron` may declare, in `unit` per cell.
///
/// Bounded above for the reason
/// [`MAX_CELLS_PER_PIXEL`](crate::image::MAX_CELLS_PER_PIXEL) is: a finite but enormous
/// scale times a terrain's extent overflows to infinity, and every figure derived from it
/// is then uncountable rather than merely large.
pub const MAX_UNITS_PER_CELL: f64 = 1.0e9;

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
    /// "leagues". Any label at all is readable here, but one
    /// [`DistanceUnit`](crate::measure::DistanceUnit) knows is what lets the scale bar
    /// step to a smaller unit as the map is zoomed; an unknown one is kept as written at
    /// every zoom.
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

    /// Write `units_per_cell` and `unit` back into the `campaign.ron` at `root`.
    ///
    /// Refuses a scale [`check`](CampaignManifest::check) would refuse, before touching
    /// the file. Fails [`CampaignError::NotACampaign`] when `root` holds no manifest and
    /// [`CampaignError::ManifestUnreadable`] or [`CampaignError::ManifestMalformed`] as
    /// [`read`](CampaignManifest::read) does, since the file is read again first.
    ///
    /// The two fields are patched in the file's own text rather than the whole manifest
    /// being re-serialized, so a field this build does not know survives — which is what
    /// [`read`](CampaignManifest::read) promises of reading and would be pointless if
    /// writing threw it away. The text is read here rather than trusted from memory, so a
    /// hand edit made while a campaign is open is not silently overwritten, and the patched
    /// text is parsed back before anything is written, so a manifest this build cannot edit
    /// in place is refused rather than mangled. The new text goes to a temporary file beside
    /// the original and is renamed over it, so a write that fails part way leaves the old
    /// manifest intact rather than a file nothing can parse.
    pub fn write_scale(root: &Path, units_per_cell: f64, unit: &str) -> Result<(), CampaignError> {
        use std::io::Write as _;

        let path = layout::manifest(root);
        Self::read(root)?;

        let text =
            std::fs::read_to_string(&path).map_err(|source| CampaignError::ManifestUnreadable {
                path: path.clone(),
                source,
            })?;
        let patched = patch_field(&text, "units_per_cell", &format!("{units_per_cell:?}"));
        let patched = patch_field(&patched, "unit", &format!("{unit:?}"));

        let round_trip: Self = ron::from_str(&patched)
            .map_err(|error| CampaignError::ManifestMalformed(error.to_string()))?;
        round_trip.check()?;
        if round_trip.units_per_cell != units_per_cell || round_trip.unit != unit {
            return Err(CampaignError::ManifestMalformed(
                "campaign.ron is not shaped in a way this build can edit in place".to_owned(),
            ));
        }

        let temporary = path.with_extension("ron.writing");
        let mut file =
            std::fs::File::create(&temporary).map_err(|source| CampaignError::LayoutUnwritable {
                path: temporary.clone(),
                source,
            })?;
        file.write_all(patched.as_bytes())
            .and_then(|()| file.sync_all())
            .map_err(|source| CampaignError::LayoutUnwritable {
                path: temporary.clone(),
                source,
            })?;
        drop(file);

        std::fs::rename(&temporary, &path).map_err(|source| {
            let _ = std::fs::remove_file(&temporary);
            CampaignError::LayoutUnwritable { path, source }
        })
    }

    fn check(&self) -> Result<(), CampaignError> {
        if self.terrain.is_empty() {
            return Err(CampaignError::ManifestMalformed(
                "terrain names no directory".to_owned(),
            ));
        }
        if let Some(reason) = crate::measure::worth_refusal(self.units_per_cell) {
            return Err(CampaignError::ManifestMalformed(format!(
                "units_per_cell is {}, which {reason}",
                self.units_per_cell
            )));
        }
        Ok(())
    }
}

fn patch_field(text: &str, field: &str, value: &str) -> String {
    let mut out = String::with_capacity(text.len() + value.len());
    let mut done = false;
    for line in text.lines() {
        let trimmed = line.trim_start();
        let names = trimmed
            .strip_prefix(field)
            .is_some_and(|rest| rest.trim_start().starts_with(':'));
        if !done && names {
            let indent = &line[..line.len() - trimmed.len()];
            let comma = match line.trim_end().ends_with(',') {
                true => ",",
                false => "",
            };
            out.push_str(&format!("{indent}{field}: {value}{comma}\n"));
            done = true;
        } else {
            out.push_str(line);
            out.push('\n');
        }
    }
    out
}
