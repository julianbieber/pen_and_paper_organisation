//! Choosing a directory by walking the tree: where a walk starts, what one directory
//! offers, and why it could not be read.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

/// The most subdirectories one listing carries.
///
/// Rows are UI entities, and a directory with tens of thousands of children would
/// stall the frame that spawns them. The excess is counted in [`Listing::unlisted`],
/// not dropped silently.
pub const MAX_LISTED: usize = 512;

/// One subdirectory a listing offers.
#[derive(Debug, Clone, PartialEq)]
pub struct Subdirectory {
    /// The directory's own name, lossily converted for display.
    pub name: String,
    /// `dir.join(file_name)` — navigable even when `name` lost information.
    pub path: PathBuf,
}

/// What one directory offers: its non-hidden subdirectories, sorted, and how many more
/// there were than [`MAX_LISTED`] would show.
#[derive(Debug, Clone, PartialEq)]
pub struct Listing {
    pub dir: PathBuf,
    pub subdirectories: Vec<Subdirectory>,
    pub unlisted: usize,
}

/// Why a directory could not be listed.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum BrowseError {
    /// `path` does not exist, is not a directory, or reading it failed outright.
    #[error("`{}` could not be read: {source}", .path.display())]
    Unreadable {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

impl BrowseError {
    /// The directory this error is about.
    pub fn path(&self) -> &Path {
        match self {
            Self::Unreadable { path, .. } => path,
        }
    }
}

/// Whether `name` is a hidden entry — starts with `.`, so it is never listed.
pub fn is_hidden(name: &OsStr) -> bool {
    name.as_encoded_bytes().first() == Some(&b'.')
}

/// The directory above `dir`, or `None` at the filesystem root.
pub fn up(dir: &Path) -> Option<PathBuf> {
    dir.parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .map(Path::to_owned)
}

/// Where a walk starts, in order: the trimmed field when it names something, then
/// `home` when it is present and non-empty, then `/`.
///
/// Pure over its inputs, like [`crate::recent::default_parent`], so this is checkable
/// without setting an environment variable.
pub fn start_candidates(field: &str, home: Option<&OsStr>) -> Vec<PathBuf> {
    let mut candidates = Vec::with_capacity(3);
    let trimmed = field.trim();
    if !trimmed.is_empty() {
        candidates.push(PathBuf::from(trimmed));
    }
    if let Some(home) = home
        && !home.is_empty()
    {
        candidates.push(PathBuf::from(home));
    }
    candidates.push(PathBuf::from(std::path::MAIN_SEPARATOR_STR));
    candidates
}

/// What `dir` holds: its non-hidden subdirectories, sorted case-insensitively and
/// capped at [`MAX_LISTED`].
///
/// A symlink to a directory is listed — [`std::fs::metadata`] follows it — and a
/// dangling one is skipped, the same as any other unreadable entry. `dir` is stored on
/// the result exactly as given, so navigating into it and back up returns here.
pub fn list(dir: &Path) -> Result<Listing, BrowseError> {
    let entries = std::fs::read_dir(dir).map_err(|source| BrowseError::Unreadable {
        path: dir.to_owned(),
        source,
    })?;

    let mut subdirectories = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name();
        if is_hidden(&name) {
            continue;
        }
        let path = dir.join(&name);
        let is_dir = std::fs::metadata(&path).is_ok_and(|meta| meta.is_dir());
        if !is_dir {
            continue;
        }
        subdirectories.push(Subdirectory {
            name: name.to_string_lossy().into_owned(),
            path,
        });
    }

    subdirectories.sort_by(|a, b| {
        a.name
            .to_lowercase()
            .cmp(&b.name.to_lowercase())
            .then_with(|| a.name.cmp(&b.name))
    });
    let unlisted = subdirectories.len().saturating_sub(MAX_LISTED);
    subdirectories.truncate(MAX_LISTED);

    Ok(Listing {
        dir: dir.to_owned(),
        subdirectories,
        unlisted,
    })
}

/// Opens a picker at the first of [`start_candidates`] that names a readable
/// directory, canonicalized.
///
/// Canonicalizing only here — never on later navigation, which is `join`/[`up`] on
/// this path — is what makes going into a symlink and back up return to where the GM
/// came from, and what makes a relative field absolute so [`up`] works from it.
pub fn open(field: &str, home: Option<&OsStr>) -> Result<Listing, BrowseError> {
    let mut last_error = None;
    for candidate in start_candidates(field, home) {
        match std::fs::canonicalize(&candidate) {
            Ok(canonical) => match std::fs::metadata(&canonical) {
                Ok(meta) if meta.is_dir() => return list(&canonical),
                Ok(_) => {
                    last_error = Some(BrowseError::Unreadable {
                        path: canonical,
                        source: std::io::Error::from(std::io::ErrorKind::NotADirectory),
                    });
                }
                Err(source) => {
                    last_error = Some(BrowseError::Unreadable {
                        path: canonical,
                        source,
                    });
                }
            },
            Err(source) => {
                last_error = Some(BrowseError::Unreadable {
                    path: candidate,
                    source,
                });
            }
        }
    }
    Err(last_error.expect("start_candidates always yields at least `/`"))
}
