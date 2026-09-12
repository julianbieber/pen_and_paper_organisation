//! The campaigns opened before, where that list is kept, and whether one of them is
//! still there.
//!
//! Disposable by design: a form this build cannot read costs the list, never the
//! ability to open a campaign, so nothing here has a version field or a migration.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::layout;

/// The directory this file sits under, joined onto a config directory.
pub const CONFIG_DIR: &str = "pnp";

/// The name of the recent-campaigns file.
pub const RECENT_FILE: &str = "recent.ron";

/// How many campaigns the file keeps, and the dialog shows — one number, so there is
/// no "remembered but not listed" state to explain.
pub const MAX_RECENT: usize = 8;

/// The largest recent file this build will read, checked before the file is read
/// rather than after.
pub const MAX_RECENT_BYTES: u64 = 64 * 1024;

/// Why the recent-campaigns file could not be read or written.
///
/// Never fatal to opening the dialog: every caller here turns this into an empty list
/// plus a status line rather than refusing to start.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum RecentError {
    /// The file is over [`MAX_RECENT_BYTES`], or reading it failed.
    #[error("`{}` could not be read: {source}", .path.display())]
    Unreadable {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    /// The file is not valid RON for [`Recents`].
    #[error("`{}` is not a recent-campaigns file: {reason}", .path.display())]
    Malformed { path: PathBuf, reason: String },
    /// The file could not be written.
    #[error("`{}` could not be written: {source}", .path.display())]
    Unwritable {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

/// One campaign this tool has opened or created before.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Recent {
    pub root: PathBuf,
    pub name: String,
}

/// The whole recent-campaigns file: newest first, deduplicated by root, capped at
/// [`MAX_RECENT`].
///
/// Carries no version field. Any form this build cannot read is recovered the same
/// way an unreadable one is — an empty list plus a status line — so a version check
/// would only buy that behaviour a field earlier.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Recents {
    pub entries: Vec<Recent>,
}

/// A remembered campaign, and whether its root still holds a manifest.
#[derive(Debug, Clone, PartialEq)]
pub struct Listed {
    pub recent: Recent,
    pub present: bool,
}

/// The config directory this tool keeps its files under, given the two environment
/// variables that decide it.
///
/// `xdg` when it is non-empty and absolute, else `<home>/.config`, else `None`. A
/// pure function over its inputs rather than a read of the process environment, so
/// this rule is checkable without setting one.
pub fn config_dir_from(xdg: Option<&OsStr>, home: Option<&OsStr>) -> Option<PathBuf> {
    let base = xdg
        .map(PathBuf::from)
        .filter(|path| !path.as_os_str().is_empty() && path.is_absolute())
        .or_else(|| home.map(|home| PathBuf::from(home).join(".config")))?;
    Some(base.join(CONFIG_DIR))
}

/// Where the recent-campaigns file is, or `None` when neither environment variable
/// gives a usable directory.
pub fn file() -> Option<PathBuf> {
    let xdg = std::env::var_os("XDG_CONFIG_HOME");
    let home = std::env::var_os("HOME");
    config_dir_from(xdg.as_deref(), home.as_deref()).map(|dir| dir.join(RECENT_FILE))
}

/// Whether `root` still holds a campaign manifest.
pub fn holds_a_campaign(root: &Path) -> bool {
    std::fs::symlink_metadata(layout::manifest(root)).is_ok_and(|meta| meta.is_file())
}

/// Where a new campaign goes by default: the directory holding `newest`, the most
/// recently opened campaign, or `home` when `newest` has no usable parent.
///
/// `newest` counts whether or not it is still present on disk — the rule is "the most
/// recently opened campaign", not "the most recently opened campaign that still
/// exists". Pure over its inputs, like [`config_dir_from`], so this is checkable
/// without touching the environment or the recent-campaigns file.
pub fn default_parent(newest: Option<&Path>, home: Option<&OsStr>) -> Option<PathBuf> {
    newest
        .and_then(Path::parent)
        .filter(|parent| !parent.as_os_str().is_empty())
        .map(Path::to_owned)
        .or_else(|| home.filter(|home| !home.is_empty()).map(PathBuf::from))
}

impl Recents {
    /// The recent-campaigns file at `path`.
    ///
    /// An absent file is an empty list, not an error — the same rule an absent
    /// `world.ron` follows. Fails [`RecentError::Unreadable`] when the file is over
    /// [`MAX_RECENT_BYTES`] or the read itself fails, and [`RecentError::Malformed`]
    /// when it is not valid RON for this type.
    pub fn read(path: &Path) -> Result<Self, RecentError> {
        let meta = match std::fs::metadata(path) {
            Ok(meta) => meta,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Self::default());
            }
            Err(source) => {
                return Err(RecentError::Unreadable {
                    path: path.to_owned(),
                    source,
                });
            }
        };
        if meta.len() > MAX_RECENT_BYTES {
            return Err(RecentError::Unreadable {
                path: path.to_owned(),
                source: std::io::Error::new(
                    std::io::ErrorKind::FileTooLarge,
                    format!("{} bytes, over the {MAX_RECENT_BYTES} byte limit", meta.len()),
                ),
            });
        }

        let text = std::fs::read_to_string(path).map_err(|source| RecentError::Unreadable {
            path: path.to_owned(),
            source,
        })?;
        ron::from_str(&text).map_err(|error| RecentError::Malformed {
            path: path.to_owned(),
            reason: error.to_string(),
        })
    }

    /// Write this list to `path`, atomically.
    ///
    /// Creates the parent directory it writes into, serializes to a `.ron.tmp`
    /// sibling, `sync_all`s it, then renames it over `path` — so a write that fails
    /// part way leaves the old file intact, or leaves no file at all, rather than one
    /// nothing can parse. A failed rename removes the temporary.
    pub fn write(&self, path: &Path) -> Result<(), RecentError> {
        use std::io::Write as _;

        let unwritable = |source: std::io::Error| RecentError::Unwritable {
            path: path.to_owned(),
            source,
        };

        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(unwritable)?;
        }

        let text = ron::ser::to_string_pretty(self, ron::ser::PrettyConfig::default())
            .map_err(|error| RecentError::Malformed {
                path: path.to_owned(),
                reason: error.to_string(),
            })?;

        let temporary = path.with_extension("ron.tmp");
        let write_result = std::fs::File::create(&temporary).and_then(|mut file| {
            file.write_all(text.as_bytes())?;
            file.sync_all()
        });
        if let Err(source) = write_result {
            let _ = std::fs::remove_file(&temporary);
            return Err(unwritable(source));
        }

        std::fs::rename(&temporary, path).map_err(|source| {
            let _ = std::fs::remove_file(&temporary);
            unwritable(source)
        })
    }

    /// Remember `root` as `name`: drop any existing entry for this root, push the new
    /// one at the front, truncate to [`MAX_RECENT`].
    ///
    /// Roots are compared as [`Path`]s and never canonicalized — the point of this
    /// list is to remember roots that may have gone, and [`Path::canonicalize`] fails
    /// on exactly those.
    pub fn remember(&mut self, root: &Path, name: &str) {
        self.entries.retain(|entry| entry.root != root);
        self.entries.insert(
            0,
            Recent {
                root: root.to_owned(),
                name: name.to_owned(),
            },
        );
        self.entries.truncate(MAX_RECENT);
    }
}

/// The remembered campaigns at `path`, each marked with whether its root still holds
/// a manifest, in the order they were remembered.
pub fn survey(path: &Path) -> Result<Vec<Listed>, RecentError> {
    let recents = Recents::read(path)?;
    Ok(recents
        .entries
        .into_iter()
        .map(|recent| {
            let present = holds_a_campaign(&recent.root);
            Listed { recent, present }
        })
        .collect())
}

/// Remember `root` as `name` in the file at `path`.
///
/// A file this build cannot read is not propagated: the list is replaced by one
/// holding just this campaign, because the alternative is a GM whose recent list
/// never recovers from a single garbage write.
pub fn record(path: &Path, root: &Path, name: &str) -> Result<(), RecentError> {
    let mut recents = Recents::read(path).unwrap_or_default();
    recents.remember(root, name);
    recents.write(path)
}
