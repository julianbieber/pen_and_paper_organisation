//! An opened campaign directory, the two ways to obtain one, and every reason
//! neither worked.
//!
//! A campaign is what is on disk: the directory, the manifest read from it, and the
//! terrain that manifest names. Session state — a selection, an undo stack, a
//! notebook handle — belongs to whoever is running the editor, not here.

use std::path::{Path, PathBuf};

use watershed::Terrain;

use crate::layout;
use crate::manifest::{
    CAMPAIGN_VERSION, CampaignManifest, DEFAULT_UNIT, DEFAULT_UNITS_PER_CELL,
};

/// Why a campaign could not be opened or created.
///
/// A missing `campaign.ron`, a version this build does not read, and a terrain
/// directory `watershed` rejects are three separate variants, so a caller can tell
/// "this is not a campaign" from "this is a campaign I am too old for" from "this
/// campaign's terrain is broken".
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum CampaignError {
    /// From `open`: the directory holds no `campaign.ron`, or what it holds under
    /// that name is not a regular file.
    #[error("`{}` is not a campaign directory: no {} in it", .0.display(), layout::MANIFEST_FILE)]
    NotACampaign(PathBuf),

    /// From `open`: `campaign.ron` is there but could not be read off the disk, or
    /// is larger than this build will read.
    #[error("`{}` is not readable: {source}", .path.display())]
    ManifestUnreadable {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    /// From `open`: `campaign.ron` was read, but is not valid RON for a
    /// [`CampaignManifest`], or carries a value the format does not admit.
    ///
    /// Distinct from [`CampaignError::ManifestUnreadable`]: the file was read fine.
    #[error("{} is malformed: {}", layout::MANIFEST_FILE, .0)]
    ManifestMalformed(String),

    /// From `open`: the manifest declares a campaign format version this build does
    /// not read. There is no migration path; the campaign is refused, not upgraded.
    #[error("campaign version {found} is not supported; this build reads {expected}")]
    UnsupportedVersion { found: u32, expected: u32 },

    /// From `open` and `create`: `watershed` refused the terrain directory the
    /// manifest names. The path is the resolved one, so a campaign written elsewhere
    /// shows where it is reaching.
    #[error("terrain at `{}` could not be read: {source}", .path.display())]
    TerrainUnreadable {
        path: PathBuf,
        #[source]
        source: watershed::IoError,
    },

    /// From `create`: the root already holds a `campaign.ron`. Nothing was written.
    #[error("`{}` already holds a campaign", .0.display())]
    AlreadyACampaign(PathBuf),

    /// From `create`: the root or one of its subdirectories could not be written.
    #[error("`{}` could not be written: {source}", .path.display())]
    LayoutUnwritable {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

/// An opened campaign directory: the root, the manifest read from it, and the
/// terrain that manifest names, loaded.
///
/// [`Campaign::open`] and [`Campaign::create`] are the only ways to obtain one, so a
/// `Campaign` in hand is one whose terrain `watershed` accepted. It is not [`Clone`]
/// on purpose — the terrain is large, and a stray copy is expensive in a way nothing
/// would complain about.
#[derive(Debug)]
pub struct Campaign {
    root: PathBuf,
    manifest: CampaignManifest,
    terrain: Terrain,
}

impl Campaign {
    /// The campaign directory at `root`.
    ///
    /// Reads and validates `campaign.ron`, then loads the terrain it names. Fails
    /// [`CampaignError::NotACampaign`], [`CampaignError::ManifestUnreadable`],
    /// [`CampaignError::ManifestMalformed`] or [`CampaignError::UnsupportedVersion`]
    /// from the manifest, and [`CampaignError::TerrainUnreadable`] when `watershed`
    /// refuses the terrain directory — including when there is no such directory.
    ///
    /// Writes nothing, and reads nothing beyond `campaign.ron` and the terrain. The
    /// terrain load is as expensive as the terrain is large and blocks until it is
    /// done; a caller with a window on screen should not run this on the thread
    /// drawing it.
    pub fn open(root: impl AsRef<Path>) -> Result<Self, CampaignError> {
        let root = root.as_ref();
        let manifest = CampaignManifest::read(root)?;
        let terrain = load_terrain(root, &manifest)?;
        Ok(Self {
            root: root.to_owned(),
            manifest,
            terrain,
        })
    }

    /// Measure this campaign at `units_per_cell` of `unit`, writing `campaign.ron` to say
    /// so.
    ///
    /// Fails as [`CampaignManifest::write_scale`] does, leaving both the file and this
    /// value as they were.
    ///
    /// The scale is the one thing about an open campaign that changes, and it changes only
    /// by being written to disk first and read back after — so this value still says what
    /// `campaign.ron` says, which is the whole of what it is for. The root and the terrain
    /// are fixed at [`open`](Campaign::open); a scale is a few bytes of manifest, and
    /// re-reading a terrain to change one would block the caller for as long as the
    /// terrain is large.
    pub fn rescale(&mut self, units_per_cell: f64, unit: &str) -> Result<(), CampaignError> {
        CampaignManifest::write_scale(&self.root, units_per_cell, unit)?;
        self.manifest = CampaignManifest::read(&self.root)?;
        Ok(())
    }

    /// Create a campaign directory at `root` recording a terrain that already sits
    /// at `terrain_dir`, and open it.
    ///
    /// The campaign is named after `root`'s own directory name, and takes
    /// [`DEFAULT_UNITS_PER_CELL`] and [`DEFAULT_UNIT`] for its scale; all three are
    /// changed by editing `campaign.ron` afterwards.
    ///
    /// Writes `root`, the `dungeons`, `images` and `notes` subdirectories, and
    /// `campaign.ron`. It **never** writes into the terrain directory, and does not
    /// copy or bake a terrain — the manifest records where one is. It does not write
    /// a world document, and does not make `notes` a `zk` notebook.
    ///
    /// The terrain is loaded before anything is written, so a `terrain_dir` that
    /// `watershed` refuses fails [`CampaignError::TerrainUnreadable`] with nothing
    /// on disk. `campaign.ron` is written last and only if it is not already there,
    /// so a failure part-way leaves no manifest and the call can simply be retried;
    /// a root that already holds one fails [`CampaignError::AlreadyACampaign`]
    /// without touching it. Subdirectories that already exist are adopted, so
    /// creating a campaign in a directory that already has content succeeds.
    pub fn create(root: impl AsRef<Path>, terrain_dir: &str) -> Result<Self, CampaignError> {
        let root = root.as_ref();
        let manifest = CampaignManifest {
            version: CAMPAIGN_VERSION,
            name: campaign_name(root),
            terrain: terrain_dir.to_owned(),
            units_per_cell: DEFAULT_UNITS_PER_CELL,
            unit: DEFAULT_UNIT.to_owned(),
        };

        let terrain = load_terrain(root, &manifest)?;

        std::fs::create_dir_all(root).map_err(|source| CampaignError::LayoutUnwritable {
            path: root.to_owned(),
            source,
        })?;
        for subdir in layout::SUBDIRS {
            let path = root.join(subdir);
            std::fs::create_dir_all(&path)
                .map_err(|source| CampaignError::LayoutUnwritable { path, source })?;
        }

        write_manifest(root, &manifest)?;

        Ok(Self {
            root: root.to_owned(),
            manifest,
            terrain,
        })
    }

    /// The directory this campaign was opened from.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// What `campaign.ron` said.
    pub fn manifest(&self) -> &CampaignManifest {
        &self.manifest
    }

    /// The terrain the manifest names, as `watershed` read it.
    pub fn terrain(&self) -> &Terrain {
        &self.terrain
    }

    /// The resolved directory the terrain was read from.
    pub fn terrain_dir(&self) -> PathBuf {
        self.manifest.terrain_dir(&self.root)
    }
}

fn load_terrain(root: &Path, manifest: &CampaignManifest) -> Result<Terrain, CampaignError> {
    let path = manifest.terrain_dir(root);
    Terrain::load_from_dir(&path).map_err(|source| CampaignError::TerrainUnreadable { path, source })
}

fn write_manifest(root: &Path, manifest: &CampaignManifest) -> Result<(), CampaignError> {
    use std::io::Write as _;

    let path = layout::manifest(root);
    let text = ron::ser::to_string_pretty(manifest, ron::ser::PrettyConfig::default())
        .map_err(|error| CampaignError::ManifestMalformed(error.to_string()))?;

    let mut file = match std::fs::File::create_new(&path) {
        Ok(file) => file,
        Err(source) if source.kind() == std::io::ErrorKind::AlreadyExists => {
            return Err(CampaignError::AlreadyACampaign(root.to_owned()));
        }
        Err(source) => return Err(CampaignError::LayoutUnwritable { path, source }),
    };
    file.write_all(text.as_bytes())
        .map_err(|source| CampaignError::LayoutUnwritable { path, source })
}

fn campaign_name(root: &Path) -> String {
    root.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "campaign".to_owned())
}
