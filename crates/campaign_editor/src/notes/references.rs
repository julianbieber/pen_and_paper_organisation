//! What references the selected place: the tag the selection asks about, the one query in
//! flight, and the answers already had.
//!
//! The query holds its own slot rather than the one [`NoteJob`](crate::notes::NoteJob)
//! uses, for the reason the `zk` probe does: a note the GM asked for must never queue
//! behind a query nothing asked for. Nothing here can refuse a note, and
//! [`refusal`](crate::notes::refusal) never learns this slot exists.
//!
//! Two rules bound the subprocesses: a tag is asked about only once it has been the
//! wanted one for [`QUERY_SETTLE`], and only one query runs at a time.
//!
//! An answer is cached under the tag it was *asked for*, never the tag wanted when it
//! lands, so a selection that moved on stores the answer rather than mis-filing it. An
//! answer asked of a notebook that has since changed is discarded instead: it describes a
//! state the watcher already invalidated.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use bevy::prelude::*;
use bevy::tasks::{IoTaskPool, Task, block_on, futures_lite::future};
use campaign::feature::FeatureId;
use campaign::notebook::{NoteError, NoteKind, Notebook, Reference, SystemRunner, tag_of};

use crate::document::WorldDoc;
use crate::features::select::Selection;
use crate::notes::ZkState;
use crate::OpenCampaign;

/// How long a tag must be the wanted one before it is worth a subprocess.
pub const QUERY_SETTLE: Duration = Duration::from_millis(200);

/// What came back for one tag.
#[derive(Debug, Clone)]
pub enum Answer {
    /// The notes carrying the tag, newest first, without the subject's own.
    Found(Vec<Reference>),
    /// `zk` was asked and refused. Cached so a broken notebook is not re-asked every
    /// frame, and carried as a message so a reader can say what happened rather than
    /// reporting an empty list that reads as "nothing references this".
    Failed(String),
}

struct InFlight {
    tag: String,
    generation: u64,
    task: Task<Result<Vec<Reference>, NoteError>>,
}

/// The selected feature's references: which tag, which answers, and the query in flight.
#[derive(Resource, Default)]
pub struct References {
    /// The one selected feature, when exactly one is selected.
    ///
    /// Kept even when that feature has no note, because "nothing is selected" and "this
    /// has no note yet" are different things to say and only this tells them apart.
    /// Written by [`follow_selection`] alone, and only where the value differs — a write
    /// per frame of a drag would re-run every reader for the length of the gesture.
    pub subject: Option<FeatureId>,
    /// The tag the subject is found by, or `None` when it has no note. Written by
    /// [`follow_selection`] alone, under the same rule as `subject`.
    pub tag: Option<String>,
    cached: HashMap<String, Answer>,
    generation: u64,
    query: Option<InFlight>,
    wanted_since: Option<Instant>,
}

impl References {
    /// Whether a query is running.
    pub fn busy(&self) -> bool {
        self.query.is_some()
    }

    /// The answer already had for the wanted tag, if any.
    pub fn answer(&self) -> Option<&Answer> {
        self.cached.get(self.tag.as_deref()?)
    }

    fn want(&mut self, subject: Option<FeatureId>, tag: Option<String>) {
        if self.subject == subject && self.tag == tag {
            return;
        }
        if self.tag != tag {
            self.wanted_since = Some(Instant::now());
        }
        self.subject = subject;
        self.tag = tag;
    }

    /// Forget every answer, and mark whatever is in flight as describing a notebook that
    /// no longer exists.
    pub fn invalidate(&mut self) {
        self.cached.clear();
        self.generation = self.generation.wrapping_add(1);
    }

    fn worth_asking(&self) -> bool {
        let Some(tag) = self.tag.as_deref() else {
            return false;
        };
        if self.cached.contains_key(tag) {
            return false;
        }
        self.wanted_since
            .is_some_and(|since| since.elapsed() >= QUERY_SETTLE)
    }
}

/// Whether the query system has anything to do, without marking the resource changed.
pub fn a_reference_query_has_work(references: Res<References>, zk: Res<ZkState>) -> bool {
    references.busy() || (zk.present && references.worth_asking())
}

/// Works out which tag the selection asks about.
///
/// A feature's note is always a place note — a note made for a feature is always made as
/// a place — so the kind is not read back out of the note's frontmatter. Nothing in this tool parses a note.
pub fn follow_selection(
    doc: Res<WorldDoc>,
    selection: Res<Selection>,
    mut references: ResMut<References>,
) {
    let subject = selection.only();
    let tag = subject
        .and_then(|id| doc.document.world().feature(id))
        .and_then(|feature| feature.note.as_deref())
        .map(|note| tag_of(NoteKind::of_a_feature(), note));

    let inner = references.bypass_change_detection();
    let before = (inner.subject, inner.tag.clone());
    inner.want(subject, tag);
    let moved = before != (inner.subject, inner.tag.clone());
    if moved {
        references.set_changed();
    }
}

/// Keeps at most one `zk list` in flight, and lands its answer in the cache.
///
/// Polls through `bypass_change_detection` for the reason
/// [`finish_note_job`](crate::notes::finish_note_job) does: a `ResMut` taken every frame
/// a query is in flight marks the resource every frame, which would make
/// [`show_references`]'s run condition true for ever and re-decide eight rows at frame
/// rate. Only a landed answer marks it.
///
/// Says nothing on the status line. Creating a note trips the watcher and so triggers a
/// query within a frame or two, and a refusal here would overwrite the "made `<path>`"
/// message that is the GM's only record of where that note went.
pub fn run_reference_query(
    campaign: Res<OpenCampaign>,
    zk: Res<ZkState>,
    mut references: ResMut<References>,
) {
    if landed(references.bypass_change_detection()) {
        references.set_changed();
        return;
    }

    let references = references.bypass_change_detection();
    if references.query.is_some() || !zk.present || !references.worth_asking() {
        return;
    }

    let tag = references.tag.clone().expect("worth_asking checked the tag");
    let subject_slug = slug_in(&tag).to_owned();
    let notebook = Notebook::of(campaign.0.root());
    let asked = tag.clone();

    references.query = Some(InFlight {
        tag,
        generation: references.generation,
        task: IoTaskPool::get().spawn(async move {
            notebook.references(&SystemRunner, &asked, &subject_slug)
        }),
    });
}

fn landed(references: &mut References) -> bool {
    if references.query.is_none() {
        return false;
    }
    let result = references
        .query
        .as_mut()
        .and_then(|query| block_on(future::poll_once(&mut query.task)));
    let Some(result) = result else {
        return false;
    };
    let InFlight { tag, generation, .. } =
        references.query.take().expect("it was there a line ago");

    if generation != references.generation {
        return false;
    }
    let answer = match result {
        Ok(found) => Answer::Found(found),
        Err(error) => {
            warn!("{error}");
            Answer::Failed(error.to_string())
        }
    };
    references.cached.insert(tag, answer);
    true
}

fn slug_in(tag: &str) -> &str {
    tag.rsplit('/').next().unwrap_or(tag)
}
