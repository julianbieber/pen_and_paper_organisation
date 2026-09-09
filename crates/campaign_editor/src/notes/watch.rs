//! Noticing that the notebook changed underneath us, so a reference the GM just wrote
//! shows up without being asked for.
//!
//! Notes are edited outside this process — that is the whole point of handing them to
//! `zk edit` — so a cached answer is stale the moment the editor is closed. A watcher is
//! what closes that gap; the Refresh button is what covers the case where it could not be
//! started.
//!
//! The watcher reports *that* something changed, never *what*: nothing here reads a
//! path's contents, and the cache is dropped wholesale rather than per tag.

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use bevy::prelude::*;
use campaign::notebook::{Notebook, ZK_DIR};
use notify::{RecommendedWatcher, RecursiveMode, Watcher};

use crate::OpenCampaign;
use crate::notes::references::References;

/// The one watcher over the campaign's notes directory, and whether it has fired.
///
/// Holding the watcher is what keeps it running: dropping a `notify` watcher stops every
/// event with nothing reported, so this field is load-bearing rather than a handle kept
/// for tidiness.
#[derive(Resource)]
pub struct NotesWatch {
    _watcher: RecommendedWatcher,
    dirty: Arc<AtomicBool>,
}

impl NotesWatch {
    /// Whether the notebook changed since this was last asked, clearing the flag.
    pub fn take_dirty(&self) -> bool {
        self.dirty.swap(false, Ordering::Relaxed)
    }
}

/// Whether an event about `path` means a note changed.
///
/// `zk list` rewrites the index under the notebook's own `.zk` directory whenever it
/// finds a note has changed, so a watcher that did not exclude it would be tripped by
/// this tool's own queries. Restricting the rest to markdown keeps an editor's swap and
/// backup files from dropping the cache on every keystroke it flushes.
pub fn a_note_changed(path: &Path) -> bool {
    if path.components().any(|part| part.as_os_str() == ZK_DIR) {
        return false;
    }
    path.extension().is_some_and(|extension| extension == "md")
}

/// Whether a watcher still has to be started.
pub fn the_notebook_is_not_watched(
    campaign: Option<Res<OpenCampaign>>,
    watch: Option<Res<NotesWatch>>,
) -> bool {
    let Some(campaign) = campaign else {
        return false;
    };
    watch.is_none() && Notebook::of(campaign.0.root()).root().is_dir()
}

/// Starts the one watcher over the campaign's notes directory.
///
/// Runs until it succeeds rather than once: a campaign opened before it had any notes has
/// no directory to watch, and `zk init` makes one the first time a note is asked for. On
/// failure nothing is inserted, so the next frame tries again — a `NotesWatch` standing
/// for "tried and gave up" would make the Refresh button the only invalidation for the
/// life of the process.
pub fn watch_notes(mut commands: Commands, campaign: Res<OpenCampaign>) {
    let root = Notebook::of(campaign.0.root()).root().to_owned();
    let dirty = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&dirty);

    let started = RecommendedWatcher::new(
        move |event: notify::Result<notify::Event>| {
            let Ok(event) = event else {
                return;
            };
            if event.paths.iter().any(|path| a_note_changed(path)) {
                flag.store(true, Ordering::Relaxed);
            }
        },
        notify::Config::default(),
    );

    let mut watcher = match started {
        Ok(watcher) => watcher,
        Err(error) => {
            warn!("the notes directory cannot be watched: {error}");
            return;
        }
    };
    if let Err(error) = watcher.watch(&root, RecursiveMode::Recursive) {
        warn!("{} cannot be watched: {error}", root.display());
        return;
    }

    commands.insert_resource(NotesWatch {
        _watcher: watcher,
        dirty,
    });
}

/// Whether the watcher has anything to report.
pub fn the_notebook_changed(watch: Option<Res<NotesWatch>>) -> bool {
    watch.is_some_and(|watch| watch.take_dirty())
}

/// Drops every cached answer once the notebook has changed.
///
/// Advancing the generation is what makes a query already in flight discardable: it was
/// asked of a notebook that no longer exists, so landing its answer would put the state
/// this just invalidated straight back into the cache.
pub fn drain_notes_watch(mut references: ResMut<References>) {
    references.invalidate();
}

#[cfg(test)]
mod tests {
    use super::*;

    // The whole reason this filter exists: `zk list` rewrites `.zk/notebook.db` whenever
    // it indexes a change, so a watcher that reported it would be tripped by this tool's
    // own queries and drop the cache it just filled.
    #[test]
    fn the_notebooks_own_index_is_not_a_note_changing() {
        assert!(!a_note_changed(Path::new(".zk/notebook.db")));
        assert!(!a_note_changed(Path::new("notes/.zk/notebook.db-wal")));
        assert!(!a_note_changed(Path::new("/tmp/c/notes/.zk/templates/place.md")));
    }

    // An editor writing a note leaves swap and backup files beside it, and each one would
    // otherwise cost a re-query while the GM is still typing.
    #[test]
    fn only_a_markdown_note_counts_as_a_change() {
        assert!(a_note_changed(Path::new("riverford-a1b2.md")));
        assert!(a_note_changed(Path::new("/tmp/c/notes/session-1-c3d4.md")));
        assert!(!a_note_changed(Path::new(".riverford-a1b2.md.swp")));
        assert!(!a_note_changed(Path::new("4913")));
        assert!(!a_note_changed(Path::new("notes")));
    }
}
