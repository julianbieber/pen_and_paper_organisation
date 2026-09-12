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
use crate::repo::{GitError, GitRunner, Repo};

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

    /// From `create`: the root already holds a `terrain` directory that is not the
    /// source being copied in. Nothing was written into it.
    #[error("`{}` already holds a terrain directory", .0.display())]
    TerrainInTheWay(PathBuf),

    /// From `create`: the source cannot be copied as it stands.
    #[error("`{}` cannot be copied: {reason}", .path.display())]
    TerrainNotCopyable { path: PathBuf, reason: &'static str },
}

/// What [`Campaign::create`] built: the campaign, and whether the directory it lives in
/// became a git repository.
///
/// Two fields rather than one: a [`Campaign`] says what is on disk and opens the same
/// way whether or not it is versioned, so whether the directory is a repository is
/// neither a field of it nor a reason to refuse the campaign — a GM who has no `git`
/// still gets a working campaign, just not a history.
#[derive(Debug)]
pub struct Created {
    pub campaign: Campaign,
    pub repository: Result<(), GitError>,
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

    /// Create a campaign directory at `root`, copying the terrain export at
    /// `terrain_source` into it, and open it.
    ///
    /// The campaign is named after `root`'s own directory name, and takes
    /// [`DEFAULT_UNITS_PER_CELL`] and [`DEFAULT_UNIT`] for its scale; all three are
    /// changed by editing `campaign.ron` afterwards.
    ///
    /// Writes `root`, the `dungeons`, `images` and `notes` subdirectories, a copy of
    /// `terrain_source` at [`layout::TERRAIN_DIR`], and `campaign.ron` naming that
    /// copy — so the directory this returns can be moved, copied or cloned and still
    /// open. It **never** writes into `terrain_source` itself. When `terrain_source`
    /// already *is* `<root>/terrain` — creating a campaign around an existing export
    /// — it is adopted rather than copied onto itself. It does not write a world
    /// document, and does not make `notes` a `zk` notebook.
    ///
    /// The terrain is loaded from `terrain_source` before anything is written, so a
    /// source `watershed` refuses fails [`CampaignError::TerrainUnreadable`] with
    /// nothing on disk. A root that already holds a `campaign.ron` fails
    /// [`CampaignError::AlreadyACampaign`] before the copy starts, so a repeated
    /// create over an existing campaign does not copy the terrain and then refuse.
    /// A `<root>/terrain` that already holds something else fails
    /// [`CampaignError::TerrainInTheWay`], and a `terrain_source` that cannot be
    /// copied — it contains `root`, or holds an entry that is neither a file nor a
    /// directory — fails [`CampaignError::TerrainNotCopyable`]; either way nothing
    /// is left under `root/terrain` for a retry to clean up. Subdirectories that
    /// already exist are adopted, so creating a campaign in a directory that already
    /// has content succeeds.
    ///
    /// Once the layout and the manifest are written, `git` runs through `Repo::at(root)`:
    /// a root already inside a work tree is adopted rather than re-initialised, and
    /// either way one commit is made so the campaign has a history from birth. That
    /// outcome is [`Created::repository`], never this function's `Err` — a GM with no
    /// `git` on `PATH` still gets a working, openable campaign, just not a repository.
    pub fn create(
        root: impl AsRef<Path>,
        terrain_source: impl AsRef<Path>,
        git: &impl GitRunner,
    ) -> Result<Created, CampaignError> {
        let root = root.as_ref();
        let terrain_source = terrain_source.as_ref();

        let terrain = Terrain::load_from_dir(terrain_source).map_err(|err| {
            CampaignError::TerrainUnreadable {
                path: terrain_source.to_owned(),
                source: err,
            }
        })?;

        if layout::manifest(root).exists() {
            return Err(CampaignError::AlreadyACampaign(root.to_owned()));
        }

        std::fs::create_dir_all(root).map_err(|source| CampaignError::LayoutUnwritable {
            path: root.to_owned(),
            source,
        })?;
        for subdir in layout::SUBDIRS {
            let path = root.join(subdir);
            std::fs::create_dir_all(&path)
                .map_err(|source| CampaignError::LayoutUnwritable { path, source })?;
        }

        copy_terrain(terrain_source, root)?;

        let manifest = CampaignManifest {
            version: CAMPAIGN_VERSION,
            name: campaign_name(root),
            terrain: layout::TERRAIN_DIR.to_owned(),
            units_per_cell: DEFAULT_UNITS_PER_CELL,
            unit: DEFAULT_UNIT.to_owned(),
        };
        write_manifest(root, &manifest)?;

        let repository = Repo::at(root).begin(git);

        Ok(Created {
            campaign: Self {
                root: root.to_owned(),
                manifest,
                terrain,
            },
            repository,
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

    /// Whether this campaign's terrain moves with its directory. See
    /// [`CampaignManifest::terrain_travels`].
    pub fn terrain_travels(&self) -> bool {
        self.manifest.terrain_travels()
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

fn copy_terrain(source: &Path, root: &Path) -> Result<(), CampaignError> {
    let destination = layout::terrain(root);

    if same_directory(source, &destination) {
        return Ok(());
    }

    if let (Ok(canonical_root), Ok(canonical_source)) =
        (std::fs::canonicalize(root), std::fs::canonicalize(source))
        && canonical_root.starts_with(&canonical_source)
    {
        return Err(CampaignError::TerrainNotCopyable {
            path: source.to_owned(),
            reason: "contains the campaign directory",
        });
    }

    std::fs::create_dir(&destination).map_err(|err| match err.kind() {
        std::io::ErrorKind::AlreadyExists => CampaignError::TerrainInTheWay(destination.clone()),
        _ => CampaignError::LayoutUnwritable {
            path: destination.clone(),
            source: err,
        },
    })?;

    if let Err(error) = copy_tree(source, &destination) {
        let _ = std::fs::remove_dir_all(&destination);
        return Err(error);
    }
    Ok(())
}

fn same_directory(a: &Path, b: &Path) -> bool {
    matches!(
        (std::fs::canonicalize(a), std::fs::canonicalize(b)),
        (Ok(a), Ok(b)) if a == b
    )
}

fn copy_tree(source: &Path, destination: &Path) -> Result<(), CampaignError> {
    let mut worklist: Vec<(PathBuf, PathBuf)> = vec![(source.to_owned(), destination.to_owned())];
    while let Some((from, to)) = worklist.pop() {
        let entries = std::fs::read_dir(&from).map_err(|source| CampaignError::LayoutUnwritable {
            path: from.clone(),
            source,
        })?;
        for entry in entries {
            let entry = entry.map_err(|source| CampaignError::LayoutUnwritable {
                path: from.clone(),
                source,
            })?;
            let file_type =
                entry
                    .file_type()
                    .map_err(|source| CampaignError::LayoutUnwritable {
                        path: entry.path(),
                        source,
                    })?;
            let to_path = to.join(entry.file_name());
            if file_type.is_dir() {
                std::fs::create_dir(&to_path).map_err(|source| CampaignError::LayoutUnwritable {
                    path: to_path.clone(),
                    source,
                })?;
                worklist.push((entry.path(), to_path));
            } else if file_type.is_file() {
                std::fs::copy(entry.path(), &to_path).map_err(|source| {
                    CampaignError::LayoutUnwritable {
                        path: to_path.clone(),
                        source,
                    }
                })?;
            } else {
                return Err(CampaignError::TerrainNotCopyable {
                    path: entry.path(),
                    reason: "is neither a file nor a directory",
                });
            }
        }
    }
    Ok(())
}

fn campaign_name(root: &Path) -> String {
    root.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "campaign".to_owned())
}
