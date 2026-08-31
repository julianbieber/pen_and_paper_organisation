//! A world under authorship: what it currently is, how to get back to what it was, and
//! whether it differs from what is on disk.
//!
//! This is session state, which is why it is not part of a [`Campaign`](crate::Campaign)
//! — that holds what is on disk and never changes. It is separate from
//! [`World`] for the same reason in the other direction: a world is the value that gets
//! written to a file, and an undo stack is not part of what a map *is*.

use std::collections::VecDeque;
use std::path::Path;

use crate::edit::{Edit, EditError};
use crate::feature::FeatureId;
use crate::world::{World, WorldError};

/// How many undone edits a document keeps.
///
/// Reaching it drops the oldest, so an old edit stops being undoable rather than a new
/// one being refused: a GM mid-stroke is never told the history is full.
pub const UNDO_LIMIT: usize = 128;

/// A world, the edits that would undo the changes made to it, and whether it has been
/// saved since.
///
/// Every change goes through [`Document::apply`], which is what makes undo and redo
/// possible at all: the inverse an [`Edit`] hands back is pushed onto a stack rather than
/// discarded. A [`Edit::Batch`] occupies one entry however many edits it carries, so a
/// brush stroke or a cascading delete undoes in one press.
#[derive(Debug)]
pub struct Document {
    world: World,
    undo: VecDeque<Edit>,
    redo: VecDeque<Edit>,
    dirty: bool,
}

impl Document {
    /// A document over `world`, with nothing to undo and nothing unsaved.
    pub fn new(world: World) -> Self {
        Self {
            world,
            undo: VecDeque::new(),
            redo: VecDeque::new(),
            dirty: false,
        }
    }

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
        &self.world
    }

    /// Whether the world differs from what was last written.
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

    /// An id no feature in this world has ever carried.
    ///
    /// Marks the document dirty, because the counter it raises is written to the file.
    /// See [`World::fresh_id`] for why raising it is not an edit and is never undone.
    pub fn fresh_id(&mut self) -> FeatureId {
        self.dirty = true;
        self.world.fresh_id()
    }

    /// Apply `edit`, keeping what would undo it.
    ///
    /// Pushes the inverse onto the undo stack, drops the oldest entry once that stack is
    /// longer than [`UNDO_LIMIT`], and clears the redo stack — a new change makes the
    /// undone future unreachable.
    ///
    /// A refused edit changes nothing at all: not the world, not either stack, not the
    /// dirty flag.
    pub fn apply(&mut self, edit: Edit) -> Result<(), EditError> {
        let inverse = edit.apply(&mut self.world)?;
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
    pub fn undo(&mut self) -> Result<bool, EditError> {
        Self::step(&mut self.world, &mut self.undo, &mut self.redo, &mut self.dirty)
    }

    /// Redo the most recently undone change, and make it undoable again.
    ///
    /// `Ok(false)` when there is nothing to redo, which is not an error.
    pub fn redo(&mut self) -> Result<bool, EditError> {
        Self::step(&mut self.world, &mut self.redo, &mut self.undo, &mut self.dirty)
    }

    /// Write the world to `path` and mark the document saved.
    ///
    /// The dirty flag is cleared only once the write has landed, so a failed save leaves
    /// the document knowing it still has something to write. Fails exactly as
    /// [`World::save`] does. The stacks are untouched: saving is not a change, and it does
    /// not cost the GM their history.
    pub fn save(&mut self, path: &Path) -> Result<(), WorldError> {
        self.world.save(path)?;
        self.dirty = false;
        Ok(())
    }

    fn step(
        world: &mut World,
        from: &mut VecDeque<Edit>,
        onto: &mut VecDeque<Edit>,
        dirty: &mut bool,
    ) -> Result<bool, EditError> {
        let Some(edit) = from.pop_back() else {
            return Ok(false);
        };
        match edit.clone().apply(world) {
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
