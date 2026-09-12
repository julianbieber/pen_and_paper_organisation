//! What a map document holds — its features and the backdrop it brings with it — the
//! file it is read from and written to, and every rule such a document must satisfy to
//! be coherent.
//!
//! A world is what the GM authored, and it is separate from the campaign that names
//! where it sits: it is loaded from and saved to a path a caller supplies, so the one
//! type serves the world map and every child document. Which of the two a document is,
//! is whether it carries a grid — the world map is drawn on the campaign's terrain and
//! carries none. Changing one is not this module's business — [`crate::edit`] owns that,
//! and is the only thing that can, through the crate-visible accessors that exist for it
//! and for nothing else.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::feature::{Feature, FeatureId, dungeon_name_refusal, note_path_refusal, reveal_refusal};
use crate::grid::{GridProblem, TileGrid};
use crate::image::{ImageBackdrop, ImageProblem};

/// The world document format this build writes, and the only one it reads.
///
/// Carried in the file and checked before the rest of it is understood, for the reason
/// [`CAMPAIGN_VERSION`](crate::manifest::CAMPAIGN_VERSION) is: there is no migration
/// path, so a document from another format is refused rather than half-read.
pub const WORLD_VERSION: u32 = 1;

/// The largest `world.ron` this build will read, checked before the file is read rather
/// than after.
pub const MAX_WORLD_BYTES: u64 = 4 * 1024 * 1024;

/// A parent link a document may not hold.
///
/// Its own type because the same two mistakes are reachable two ways — reading a
/// document off disk, and applying an edit to one already open — and a reader should get
/// the same sentence either way. [`WorldError`] and [`crate::edit::EditError`] both carry
/// it rather than restating it.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ParentProblem {
    /// A feature names a parent no feature in the document carries.
    #[error("{child} names {parent} as its parent, and there is no such feature")]
    Dangling { child: FeatureId, parent: FeatureId },

    /// Following parents from somewhere in the document arrives back at this feature.
    #[error("{feature} is inside itself: its parents lead back to it")]
    Cycle { feature: FeatureId },
}

/// Why a world document could not be read, or refused once it had been.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum WorldError {
    /// The path holds something that is not a regular file, or one that could not be
    /// read off the disk, or one larger than this build will read.
    ///
    /// Distinct from there being nothing at the path at all, which is not an error: an
    /// absent world document is an empty world.
    #[error("`{}` is not readable: {source}", .path.display())]
    WorldUnreadable {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    /// The file was read, but is not valid RON for a [`World`].
    #[error("`{}` is malformed: {reason}", .path.display())]
    WorldMalformed { path: PathBuf, reason: String },

    /// The document declares a format version this build does not read. There is no
    /// migration path; it is refused, not upgraded.
    #[error("world document version {found} is not supported; this build reads {expected}")]
    UnsupportedVersion { found: u32, expected: u32 },

    /// The document's parent links do not describe a forest.
    #[error("`{}` has a broken parent link: {source}", .path.display())]
    BadParent {
        path: PathBuf,
        #[source]
        source: ParentProblem,
    },

    /// A feature carries fewer vertices than its shape admits.
    #[error("{feature} is a {shape} carrying {have} vertices, and a {shape} needs {least}")]
    DegenerateGeometry {
        feature: FeatureId,
        shape: &'static str,
        have: usize,
        least: usize,
    },

    /// A feature carries a coordinate that is not finite. Refused rather than stored,
    /// because a coordinate that cannot be compared with itself would make a document
    /// unable to round-trip.
    #[error("{feature} carries a coordinate that is not a finite number")]
    NonFiniteCoordinate { feature: FeatureId },

    /// A feature carries a note path a feature may not carry.
    #[error("{feature} has a note path that {reason}")]
    BadNotePath {
        feature: FeatureId,
        reason: &'static str,
    },

    /// A feature carries a reveal threshold that is not a finite positive number.
    ///
    /// Refused for the reason a non-finite coordinate is: the threshold is compared
    /// against the map's scale every frame, and one that cannot be compared would hide
    /// the feature for the life of the document with nothing to say why.
    #[error("{feature} has a reveal scale of {scale}, which {reason}")]
    BadRevealScale {
        feature: FeatureId,
        scale: f32,
        reason: &'static str,
    },

    /// A feature carries a dungeon name a feature may not carry.
    #[error("{feature} has a dungeon name that {reason}")]
    BadDungeonName {
        feature: FeatureId,
        reason: &'static str,
    },

    /// A feature inside a document that has a grid names a dungeon of its own.
    ///
    /// A dungeon is a child of the world map and not of another dungeon: there is one
    /// place to come back to, so opening a dungeon from inside one would have nowhere to
    /// park what it left. Refused where it is stored rather than where it would be
    /// opened, so the rule is checkable without a window.
    #[error("{feature} is inside a dungeon and names a dungeon of its own, which nests")]
    NestedDungeon { feature: FeatureId },

    /// Two features name the same dungeon document.
    #[error("{first} and {second} both name the dungeon `{name}`")]
    SharedDungeon {
        first: FeatureId,
        second: FeatureId,
        name: String,
    },

    /// The document's grid is not one a document may hold.
    #[error("`{}` has a grid that cannot be drawn: {source}", .path.display())]
    BadGrid {
        path: PathBuf,
        #[source]
        source: GridProblem,
    },

    /// The document's image backdrop is not one a document may hold.
    ///
    /// About the *declaration* and never about the file: a document naming a picture that
    /// is missing or unreadable opens perfectly well and draws its features, because
    /// nothing here reads the disk. Only a declaration that could not be drawn whatever
    /// the file turned out to be is refused.
    #[error("`{}` has an image backdrop that cannot be drawn: {source}", .path.display())]
    BadImage {
        path: PathBuf,
        #[source]
        source: ImageProblem,
    },

    /// The document could not be written.
    #[error("`{}` could not be written: {source}", .path.display())]
    WorldUnwritable {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    /// The document would serialize to more than this build reads back.
    ///
    /// Refused at the point of writing rather than discovered on the next open, because a
    /// save that reports success and then cannot be reopened is indistinguishable from
    /// losing the work.
    #[error(
        "`{}` would be {bytes} bytes, over the {limit} byte limit this build reads back",
        .path.display()
    )]
    WorldTooLarge {
        path: PathBuf,
        bytes: u64,
        limit: u64,
    },
}

#[derive(Deserialize)]
#[serde(rename = "World")]
struct WorldVersion {
    version: u32,
}

/// Every feature a map holds, and the counter that names the next one.
///
/// The feature set changes only by applying an [`Edit`](crate::edit::Edit) — the fields
/// are private and there is no other way in, so a button and a control socket cannot
/// come to mean different things.
///
/// `next_id` is the exception, and deliberately so: it is an allocator, not content.
/// [`World::fresh_id`] raises it, and no edit and no inverse ever moves it. That is what
/// makes an id permanent — undoing the add of a feature does not hand its id back out —
/// and it is also what keeps an inverse exact, since applying an edit and then its
/// inverse leaves every field of the world untouched.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct World {
    version: u32,
    next_id: u64,
    features: BTreeMap<FeatureId, Feature>,
    /// The backdrop this document is drawn on, when it brings its own.
    ///
    /// Absent on the world map, whose backdrop is the campaign's terrain, and present on
    /// a dungeon, which has no meaningful position at terrain-cell scale and so carries
    /// the grid it is drawn on. Defaulted and skipped when absent, so a `world.ron`
    /// written before dungeons existed still reads and one written now still opens in a
    /// build that has never heard of a grid.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    grid: Option<TileGrid>,
    /// The picture this document is drawn over, when the GM has imported one.
    ///
    /// Independent of the grid: a dungeon may be drawn on its grid with a scan laid over
    /// it, and the world map may carry one just as well. Defaulted and skipped when absent
    /// for the reason the grid is, so a `world.ron` written before backdrops existed still
    /// reads and one written now still opens in a build that has never heard of one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    image: Option<ImageBackdrop>,
}

impl Default for World {
    fn default() -> Self {
        Self {
            version: WORLD_VERSION,
            next_id: 0,
            features: BTreeMap::new(),
            grid: None,
            image: None,
        }
    }
}

impl World {
    /// The world document at `path`, or an empty world when there is nothing there.
    ///
    /// An absent file is not an error: a campaign is created without a world document,
    /// so "missing" reads as "empty" rather than needing a migration. Something at the
    /// path that is *not* a regular file is a different matter and fails
    /// [`WorldError::WorldUnreadable`] — reading a FIFO as "absent" would hang.
    ///
    /// Fails [`WorldError::UnsupportedVersion`] for a format this build does not read,
    /// [`WorldError::WorldMalformed`] for anything RON refuses, and — once it parses —
    /// [`WorldError::DegenerateGeometry`], [`WorldError::NonFiniteCoordinate`],
    /// [`WorldError::BadNotePath`] or [`WorldError::BadParent`] for a document that
    /// parses but is not coherent. Every one of them names the offending feature.
    ///
    /// Writes nothing.
    pub fn load(path: &Path) -> Result<Self, WorldError> {
        let text = match Self::read_to_string(path)? {
            Some(text) => text,
            None => return Ok(Self::default()),
        };
        Self::from_ron(&text, path)
    }

    /// Write this world to `path`, replacing whatever is there.
    ///
    /// The bytes land in a sibling temporary file which is then renamed over the target,
    /// so a failure part-way leaves the document that was already there intact. A world
    /// document is the only copy of everything the GM has drawn, and truncating it in
    /// place is the one unrecoverable way to lose that.
    ///
    /// The temporary is opened with `create_new` and named with this process's id, so a
    /// symlink sitting where it would go is refused rather than followed and written
    /// through, and two campaigns open at once cannot stage over one another. The rename
    /// over `path` itself is safe either way — a rename replaces a symlink and does not
    /// follow it. A leftover temporary from a previous crash is removed first, so the
    /// stricter open cannot make saving fail for ever.
    ///
    /// Creates `path`'s parent directory first, if it is not there: git does not carry
    /// an empty directory, so a clone that never held a dungeon must still be able to
    /// save one into `dungeons/`.
    ///
    /// Refuses [`WorldError::WorldTooLarge`] for a world that would serialize to more than
    /// [`MAX_WORLD_BYTES`], so that a world this build writes is always one it reads back.
    /// Nothing is written in that case — the document already on disk is left alone.
    ///
    /// Fails [`WorldError::WorldUnwritable`] naming `path`.
    pub fn save(&self, path: &Path) -> Result<(), WorldError> {
        use std::io::Write as _;

        let unwritable = |source: std::io::Error| WorldError::WorldUnwritable {
            path: path.to_owned(),
            source,
        };

        let text = self.to_ron().map_err(|error| WorldError::WorldUnwritable {
            path: path.to_owned(),
            source: std::io::Error::other(error),
        })?;

        if text.len() as u64 > MAX_WORLD_BYTES {
            return Err(WorldError::WorldTooLarge {
                path: path.to_owned(),
                bytes: text.len() as u64,
                limit: MAX_WORLD_BYTES,
            });
        }

        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent).map_err(unwritable)?;
        }

        let temporary = temporary_beside(path);
        let _ = std::fs::remove_file(&temporary);
        let mut file = std::fs::File::create_new(&temporary).map_err(unwritable)?;
        file.write_all(text.as_bytes()).map_err(unwritable)?;
        file.sync_all().map_err(unwritable)?;
        drop(file);

        std::fs::rename(&temporary, path).map_err(|source| {
            let _ = std::fs::remove_file(&temporary);
            unwritable(source)
        })
    }

    /// The world this RON text describes, checked as [`World::load`] checks it.
    ///
    /// `path` appears in the error messages and is not read. Separate from
    /// [`World::load`] so that the rules a document must satisfy can be exercised
    /// without a directory to put one in.
    pub fn from_ron(text: &str, path: &Path) -> Result<Self, WorldError> {
        let malformed = |error: ron::error::SpannedError| WorldError::WorldMalformed {
            path: path.to_owned(),
            reason: error.to_string(),
        };

        let probe: WorldVersion = ron::from_str(text).map_err(malformed)?;
        if probe.version != WORLD_VERSION {
            return Err(WorldError::UnsupportedVersion {
                found: probe.version,
                expected: WORLD_VERSION,
            });
        }

        let mut world: Self = ron::from_str(text).map_err(malformed)?;
        world.check(path)?;
        world.raise_next_id();
        Ok(world)
    }

    /// This world as the RON text [`World::save`] writes.
    ///
    /// Deterministic: features are held in id order, so two calls on one world produce
    /// identical bytes.
    pub fn to_ron(&self) -> Result<String, ron::Error> {
        ron::ser::to_string_pretty(self, ron::ser::PrettyConfig::default())
    }

    /// The document format version this world was read at, or will be written at.
    pub fn version(&self) -> u32 {
        self.version
    }

    /// How many features this world holds.
    pub fn len(&self) -> usize {
        self.features.len()
    }

    /// Whether this world holds no features at all, which is what an absent document
    /// loads as.
    pub fn is_empty(&self) -> bool {
        self.features.is_empty()
    }

    /// A world drawn on `grid`, holding no features.
    ///
    /// What a dungeon is created as. The world map is [`World::default`], which carries
    /// no grid because its backdrop is the campaign's terrain.
    pub fn on_a_grid(grid: TileGrid) -> Self {
        Self {
            grid: Some(grid),
            ..Self::default()
        }
    }

    /// The grid this document is drawn on, or `None` when its backdrop is the terrain.
    pub fn grid(&self) -> Option<&TileGrid> {
        self.grid.as_ref()
    }

    /// The picture this document is drawn over, or `None` when it declares none.
    pub fn image(&self) -> Option<&ImageBackdrop> {
        self.image.as_ref()
    }

    /// The feature `id` names, if this world holds one.
    pub fn feature(&self, id: FeatureId) -> Option<&Feature> {
        self.features.get(&id)
    }

    /// Every feature, in id order — the order they are written in, so a caller iterating
    /// them sees the same sequence a saved document does.
    pub fn features(&self) -> impl Iterator<Item = (FeatureId, &Feature)> {
        self.features.iter().map(|(id, feature)| (*id, feature))
    }

    /// Every feature naming `id` as its parent, in id order.
    ///
    /// The buildings inside a settlement, in other words. Empty for a feature nothing
    /// hangs off, and empty for an id this world does not hold.
    pub fn children_of(&self, id: FeatureId) -> impl Iterator<Item = FeatureId> {
        self.features
            .iter()
            .filter(move |(_, feature)| feature.parent == Some(id))
            .map(|(child, _)| *child)
    }

    /// An id no feature in this world has ever carried, raising the counter that says so.
    ///
    /// This is the one change to a world that is not an [`Edit`](crate::edit::Edit), and
    /// it is never undone: an id handed out stays spent even if the feature built with it
    /// is deleted, or the add that used it is undone. That is what lets a note or a child
    /// document hold an id and keep meaning the feature it meant.
    pub fn fresh_id(&mut self) -> FeatureId {
        let id = FeatureId(self.next_id);
        self.next_id = self.next_id.saturating_add(1);
        id
    }

    /// Whether `child` may take `parent`, given the features this world currently holds.
    ///
    /// The same question [`World::load`] asks of a document it has just read, so an edit
    /// can never build a world that would be refused if it were saved and opened again.
    /// `None` for `parent` is always fine.
    pub fn may_take_parent(
        &self,
        child: FeatureId,
        parent: Option<FeatureId>,
    ) -> Result<(), ParentProblem> {
        let Some(parent) = parent else {
            return Ok(());
        };
        if !self.features.contains_key(&parent) {
            return Err(ParentProblem::Dangling { child, parent });
        }
        if parent == child {
            return Err(ParentProblem::Cycle { feature: child });
        }

        let mut walked = 0usize;
        let mut current = Some(parent);
        while let Some(id) = current {
            if id == child {
                return Err(ParentProblem::Cycle { feature: child });
            }
            walked += 1;
            if walked > self.features.len() {
                return Err(ParentProblem::Cycle { feature: id });
            }
            current = self.features.get(&id).and_then(|feature| feature.parent);
        }
        Ok(())
    }

    pub(crate) fn features_mut(&mut self) -> &mut BTreeMap<FeatureId, Feature> {
        &mut self.features
    }

    pub(crate) fn grid_mut(&mut self) -> Option<&mut TileGrid> {
        self.grid.as_mut()
    }

    pub(crate) fn image_mut(&mut self) -> &mut Option<ImageBackdrop> {
        &mut self.image
    }

    fn read_to_string(path: &Path) -> Result<Option<String>, WorldError> {
        let unreadable = |source: std::io::Error| WorldError::WorldUnreadable {
            path: path.to_owned(),
            source,
        };

        let meta = match std::fs::metadata(path) {
            Ok(meta) => meta,
            Err(source) if source.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(source) => return Err(unreadable(source)),
        };
        if !meta.is_file() {
            return Err(unreadable(std::io::Error::other(
                "not a regular file, so it is refused rather than read as an empty world",
            )));
        }

        let len = meta.len();
        if len > MAX_WORLD_BYTES {
            return Err(unreadable(std::io::Error::new(
                std::io::ErrorKind::FileTooLarge,
                format!("{len} bytes, over the {MAX_WORLD_BYTES} byte limit"),
            )));
        }

        std::fs::read_to_string(path).map(Some).map_err(unreadable)
    }

    fn check(&self, path: &Path) -> Result<(), WorldError> {
        if let Some(grid) = &self.grid
            && let Some(source) = grid.refusal()
        {
            return Err(WorldError::BadGrid {
                path: path.to_owned(),
                source,
            });
        }

        if let Some(image) = &self.image
            && let Some(source) = image.refusal()
        {
            return Err(WorldError::BadImage {
                path: path.to_owned(),
                source,
            });
        }

        let mut dungeons: BTreeMap<&str, FeatureId> = BTreeMap::new();
        for (id, feature) in &self.features {
            if let Some(name) = feature.dungeon.as_deref() {
                if let Some(reason) = dungeon_name_refusal(name) {
                    return Err(WorldError::BadDungeonName {
                        feature: *id,
                        reason,
                    });
                }
                if self.grid.is_some() {
                    return Err(WorldError::NestedDungeon { feature: *id });
                }
                if let Some(first) = dungeons.insert(name, *id) {
                    return Err(WorldError::SharedDungeon {
                        first,
                        second: *id,
                        name: name.to_owned(),
                    });
                }
            }
        }

        for (id, feature) in &self.features {
            if !feature.geometry.is_finite() {
                return Err(WorldError::NonFiniteCoordinate { feature: *id });
            }
            if !feature.geometry.has_enough_vertices() {
                return Err(WorldError::DegenerateGeometry {
                    feature: *id,
                    shape: feature.geometry.shape(),
                    have: feature.geometry.len(),
                    least: feature.geometry.least(),
                });
            }
            if let Some(note) = feature.note.as_deref()
                && let Some(reason) = note_path_refusal(note)
            {
                return Err(WorldError::BadNotePath {
                    feature: *id,
                    reason,
                });
            }
            if let Some(scale) = feature.max_cells_per_pixel
                && let Some(reason) = reveal_refusal(scale)
            {
                return Err(WorldError::BadRevealScale {
                    feature: *id,
                    scale,
                    reason,
                });
            }
        }

        self.check_parents().map_err(|source| WorldError::BadParent {
            path: path.to_owned(),
            source,
        })
    }

    fn check_parents(&self) -> Result<(), ParentProblem> {
        for (child, feature) in &self.features {
            if let Some(parent) = feature.parent
                && !self.features.contains_key(&parent)
            {
                return Err(ParentProblem::Dangling {
                    child: *child,
                    parent,
                });
            }
        }

        let mut mark: BTreeMap<FeatureId, Mark> = BTreeMap::new();
        let mut walk: Vec<FeatureId> = Vec::new();

        for start in self.features.keys() {
            let mut current = Some(*start);
            while let Some(id) = current {
                match mark.get(&id) {
                    Some(Mark::Settled) => break,
                    Some(Mark::InProgress) => return Err(ParentProblem::Cycle { feature: id }),
                    None => {
                        mark.insert(id, Mark::InProgress);
                        walk.push(id);
                        current = self.features[&id].parent;
                    }
                }
            }
            for id in walk.drain(..) {
                mark.insert(id, Mark::Settled);
            }
        }
        Ok(())
    }

    fn raise_next_id(&mut self) {
        let largest = self.features.keys().next_back().map_or(0, |id| id.0 + 1);
        self.next_id = self.next_id.max(largest);
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Mark {
    InProgress,
    Settled,
}

fn temporary_beside(path: &Path) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_owned();
    name.push(format!(".{}.tmp", std::process::id()));
    path.with_file_name(name)
}
