//! A value under authorship: what it currently is, how to get back to what it was, and
//! whether it differs from what is on disk.
//!
//! This is session state, which is why it is not part of a [`Campaign`](crate::Campaign)
//! — that holds what is on disk and never changes. It is separate from the value it
//! authors for the same reason in the other direction: a world is the value that gets
//! written to a file, and an undo stack is not part of what a map *is*.

use std::collections::VecDeque;
use std::fmt::Debug;
use std::path::Path;

use crate::combat::CombatMap;
use crate::edit::{Edit, EditError};
use crate::feature::FeatureId;
use crate::world::{World, WorldError};

/// How many undone edits a document keeps.
///
/// Reaching it drops the oldest, so an old edit stops being undoable rather than a new
/// one being refused: a GM mid-stroke is never told the history is full.
pub const UNDO_LIMIT: usize = 128;

/// A value a [`Document`] can author: one that changes only by applying an edit, and
/// that hands back the edit undoing it.
///
/// The inverse must be exact — applying an edit and then what it returned leaves the
/// value as it was — and a refused edit must change nothing, or undo and redo stop
/// meaning what they say.
pub trait Authored {
    /// The change this value is made by.
    type Edit: Clone + Debug;

    /// Why an edit was refused.
    type Refusal;

    /// Apply `edit` and hand back the edit that undoes it, or refuse it having changed
    /// nothing.
    fn apply_edit(&mut self, edit: Self::Edit) -> Result<Self::Edit, Self::Refusal>;
}

impl Authored for World {
    type Edit = Edit;
    type Refusal = EditError;

    fn apply_edit(&mut self, edit: Edit) -> Result<Edit, EditError> {
        edit.apply(self)
    }
}

/// A value, the edits that would undo the changes made to it, and whether it has been
/// saved since.
///
/// Every change goes through [`Document::apply`], which is what makes undo and redo
/// possible at all: the inverse an edit hands back is pushed onto a stack rather than
/// discarded. A world's [`Edit::Batch`] occupies one entry however many edits it
/// carries, so a brush stroke or a cascading delete undoes in one press.
#[derive(Debug)]
pub struct Document<D: Authored = World> {
    content: D,
    undo: VecDeque<D::Edit>,
    redo: VecDeque<D::Edit>,
    dirty: bool,
}

impl<D: Authored> Document<D> {
    /// A document over `content`, with nothing to undo and nothing unsaved.
    pub fn new(content: D) -> Self {
        Self {
            content,
            undo: VecDeque::new(),
            redo: VecDeque::new(),
            dirty: false,
        }
    }

    /// The value as it currently stands.
    ///
    /// Read-only on purpose: [`Document::apply`] is the only way to change it, so nothing
    /// can make a change the undo stack does not know about.
    pub fn content(&self) -> &D {
        &self.content
    }

    /// Whether the value differs from what was last written.
    ///
    /// True from the first change until a save lands. Undo and redo are changes for this
    /// purpose: what is on disk is what was last written, not wherever the stacks have
    /// since been wound to.
    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    /// How many changes could be undone. A batch counts once.
    pub fn undo_depth(&self) -> usize {
        self.undo.len()
    }

    /// How many undone changes could be redone. A batch counts once.
    pub fn redo_depth(&self) -> usize {
        self.redo.len()
    }

    /// Apply `edit`, keeping what would undo it.
    ///
    /// Pushes the inverse onto the undo stack, drops the oldest entry once that stack is
    /// longer than [`UNDO_LIMIT`], and clears the redo stack — a new change makes the
    /// undone future unreachable.
    ///
    /// A refused edit changes nothing at all: not the value, not either stack, not the
    /// dirty flag.
    pub fn apply(&mut self, edit: D::Edit) -> Result<(), D::Refusal> {
        let inverse = self.content.apply_edit(edit)?;
        self.undo.push_back(inverse);
        if self.undo.len() > UNDO_LIMIT {
            self.undo.pop_front();
        }
        self.redo.clear();
        self.dirty = true;
        Ok(())
    }

    /// Undo the most recent change, and make it redoable.
    ///
    /// `Ok(false)` when there is nothing to undo, which is not an error. Deliberately not
    /// routed through [`Document::apply`], which clears the redo stack — doing so here
    /// would discard the entry this call has just pushed onto it.
    pub fn undo(&mut self) -> Result<bool, D::Refusal> {
        Self::step(&mut self.content, &mut self.undo, &mut self.redo, &mut self.dirty)
    }

    /// Redo the most recently undone change, and make it undoable again.
    ///
    /// `Ok(false)` when there is nothing to redo, which is not an error.
    pub fn redo(&mut self) -> Result<bool, D::Refusal> {
        Self::step(&mut self.content, &mut self.redo, &mut self.undo, &mut self.dirty)
    }

    fn step(
        content: &mut D,
        from: &mut VecDeque<D::Edit>,
        onto: &mut VecDeque<D::Edit>,
        dirty: &mut bool,
    ) -> Result<bool, D::Refusal> {
        let Some(edit) = from.pop_back() else {
            return Ok(false);
        };
        match content.apply_edit(edit.clone()) {
            Ok(inverse) => {
                onto.push_back(inverse);
                if onto.len() > UNDO_LIMIT {
                    onto.pop_front();
                }
                *dirty = true;
                Ok(true)
            }
            Err(refusal) => {
                from.push_back(edit);
                Err(refusal)
            }
        }
    }
}

impl Document<World> {
    /// The document at `path`, or an empty one when there is nothing there.
    ///
    /// Fails exactly as [`World::load`] does.
    pub fn load(path: &Path) -> Result<Self, WorldError> {
        World::load(path).map(Self::new)
    }

    /// The world as it currently stands.
    ///
    /// Read-only on purpose: [`Document::apply`] is the only way to change it, so nothing
    /// can make a change the undo stack does not know about.
    pub fn world(&self) -> &World {
        &self.content
    }

    /// An id no feature in this world has ever carried.
    ///
    /// Marks the document dirty, because the counter it raises is written to the file.
    /// See [`World::fresh_id`] for why raising it is not an edit and is never undone.
    pub fn fresh_id(&mut self) -> FeatureId {
        self.dirty = true;
        self.content.fresh_id()
    }

    /// Write the world to `path` and mark the document saved.
    ///
    /// The dirty flag is cleared only once the write has landed, so a failed save leaves
    /// the document knowing it still has something to write. Fails exactly as
    /// [`World::save`] does. The stacks are untouched: saving is not a change, and it does
    /// not cost the GM their history.
    pub fn save(&mut self, path: &Path) -> Result<(), WorldError> {
        self.content.save(path)?;
        self.dirty = false;
        Ok(())
    }
}

impl Document<CombatMap> {
    /// Write the map to `path` and mark the document saved.
    ///
    /// The dirty flag is cleared only once the write has landed, so a failed save leaves
    /// the document knowing it still has something to write. Fails exactly as
    /// [`CombatMap::save`] does. The stacks are untouched.
    pub fn save(&mut self, path: &Path) -> Result<(), WorldError> {
        self.content.save(path)?;
        self.dirty = false;
        Ok(())
    }
}
