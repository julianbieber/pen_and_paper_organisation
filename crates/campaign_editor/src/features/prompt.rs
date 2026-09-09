//! The two questions authoring has to ask before it can proceed, and the close it holds
//! back until one of them is answered.
//!
//! The sheet is built once and hidden, and raised by toggling its display — the idiom the
//! open dialog already uses. Spawning and despawning a modal instead would re-run the
//! whole UI layout every time a question is raised.
//!
//! The app closes its own window, so `guard_close` here is one half of a pair: it answers
//! a close request while a document exists, and `close_without_a_document` in the shell
//! answers one in every other state. A state that answers neither is a window that cannot
//! be shut.

use bevy::feathers::controls::FeathersButton;
use bevy::feathers::theme::{ThemeBackgroundColor, ThemedText};
use bevy::feathers::tokens;
use bevy::prelude::*;
use bevy::ui_widgets::Activate;
use bevy::window::WindowCloseRequested;
use campaign::FeatureId;
use campaign::gesture::{self, Orphans};

use crate::document::{self as doc, WorldDoc};
use crate::features::select::Selection;
use crate::StatusMessage;

/// What is being asked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Question {
    /// The window was asked to close while the document is unsaved.
    UnsavedOnClose,
    /// A feature is being deleted and something outside the selection hangs off it.
    OrphansOnDelete,
}

/// One answer to one question.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Answer {
    Save,
    Discard,
    Cascade,
    Promote,
    Cancel,
}

/// The question on screen, if there is one, and what it is about.
///
/// One slot: a second question raised over the first would replace it, and the answer
/// given would be to a question the GM is no longer looking at. `subject` means something
/// only for [`Question::OrphansOnDelete`].
#[derive(Resource, Debug, Default)]
pub struct Asking {
    pub question: Option<Question>,
    pub subject: Option<FeatureId>,
}

impl Asking {
    /// Raise `question`, unless one is already standing.
    pub fn raise(&mut self, question: Question, subject: Option<FeatureId>) {
        if self.question.is_some() {
            return;
        }
        self.question = Some(question);
        self.subject = subject;
    }

    /// Take the question away.
    pub fn settle(&mut self) {
        self.question = None;
        self.subject = None;
    }
}

/// The modal sheet, built once and shown by toggling its display.
#[derive(Component, Default, Clone)]
pub struct PromptRoot;

/// The row of answers belonging to one question.
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct PromptRow {
    pub question: Question,
}

/// One answer button.
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct PromptButton {
    pub answer: Answer,
}

/// Whether a question is on screen.
///
/// While one is, nothing authors and the camera does not move, so the answer cannot be
/// given against a map that shifted underneath it.
pub fn a_question_is_up(asking: Option<Res<Asking>>) -> bool {
    asking.is_some_and(|asking| asking.question.is_some())
}

/// Builds the modal sheet, hidden, with a row of answers per question.
pub fn build_prompt(mut commands: Commands) {
    commands.spawn_scene(sheet());
}

/// Turns a close request on an unsaved document into a question rather than an exit.
///
/// The window was built refusing to close itself, so this owns the exit while a document
/// exists.
pub fn guard_close(
    mut requests: MessageReader<WindowCloseRequested>,
    doc: Res<WorldDoc>,
    mut asking: ResMut<Asking>,
    mut exit: MessageWriter<AppExit>,
) {
    if requests.read().next().is_none() {
        return;
    }
    if doc.anything_unsaved() {
        asking.raise(Question::UnsavedOnClose, None);
    } else {
        exit.write(AppExit::Success);
    }
}

/// Shows the row of answers for the pending question, and hides the sheet when there is
/// none.
pub fn show_prompt(
    asking: Res<Asking>,
    mut sheets: Query<&mut Node, (With<PromptRoot>, Without<PromptRow>)>,
    mut rows: Query<(&PromptRow, &mut Node), Without<PromptRoot>>,
) {
    for mut node in sheets.iter_mut() {
        let display = if asking.question.is_some() {
            Display::Flex
        } else {
            Display::None
        };
        if node.display != display {
            node.display = display;
        }
    }
    for (row, mut node) in rows.iter_mut() {
        let display = if asking.question == Some(row.question) {
            Display::Flex
        } else {
            Display::None
        };
        if node.display != display {
            node.display = display;
        }
    }
}

/// Answers the standing question with `answer`.
///
/// Shared by the buttons' observers and by Escape, which answers [`Answer::Cancel`] — a
/// modal that can only be dismissed with the mouse is one the GM has to reach for.
///
/// Takes the question away whatever the answer. [`Answer::Cancel`], and any answer
/// belonging to the other question, then change nothing else — so an answer can never be
/// applied to a question it was not offered for.
pub fn answer(
    answer: Answer,
    asking: &mut Asking,
    doc: &mut WorldDoc,
    selection: &mut Selection,
    status: &mut StatusMessage,
    exit: &mut MessageWriter<AppExit>,
) {
    let Some(question) = asking.question else {
        return;
    };
    let subject = asking.subject;
    asking.settle();

    match (question, answer) {
        (Question::UnsavedOnClose, Answer::Save) => match doc.save().and_then(|()| doc.save_parked()) {
            Ok(()) => {
                exit.write(AppExit::Success);
            }
            Err(error) => {
                error!("{error}");
                status.say(error.to_string());
            }
        },
        (Question::UnsavedOnClose, Answer::Discard) => {
            exit.write(AppExit::Success);
        }
        (Question::OrphansOnDelete, Answer::Cascade | Answer::Promote) => {
            let Some(id) = subject else {
                return;
            };
            let orphans = if answer == Answer::Cascade {
                Orphans::Cascade
            } else {
                Orphans::Promote
            };
            let edit = gesture::remove(doc.document.world(), id, orphans);
            if doc::apply(doc, status, edit) {
                selection.clear();
            }
        }
        _ => {}
    }
}

fn sheet() -> impl Scene {
    bsn! {
        Node {
            position_type: PositionType::Absolute,
            width: percent(100),
            height: percent(100),
            display: Display::None,
            flex_direction: FlexDirection::Column,
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            row_gap: px(10),
            padding: px(20),
        }
        GlobalZIndex(50)
        ThemeBackgroundColor(tokens::WINDOW_BG)
        PromptRoot
        Children [
            row(
                Question::UnsavedOnClose,
                "This campaign has unsaved changes.",
                [("Save and close", Answer::Save),
                 ("Close without saving", Answer::Discard),
                 ("Cancel", Answer::Cancel)]
            ),
            row(
                Question::OrphansOnDelete,
                "Other features sit inside this one.",
                [("Delete them too", Answer::Cascade),
                 ("Keep them, move them out", Answer::Promote),
                 ("Cancel", Answer::Cancel)]
            )
        ]
    }
}

fn row(question: Question, caption: &'static str, answers: [(&'static str, Answer); 3]) -> impl Scene {
    bsn! {
        Node {
            display: Display::None,
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Center,
            row_gap: px(10),
        }
        PromptRow { question: {question} }
        Children [
            (Text({caption.to_string()}) ThemedText),
            (
                Node {
                    display: Display::Flex,
                    flex_direction: FlexDirection::Row,
                    column_gap: px(8),
                }
                Children [
                    answer_button(answers[0].0, answers[0].1),
                    answer_button(answers[1].0, answers[1].1),
                    answer_button(answers[2].0, answers[2].1)
                ]
            )
        ]
    }
}

fn answer_button(caption: &'static str, answer: Answer) -> impl Scene {
    bsn! {
        @FeathersButton {
            @caption: bsn! { Text({caption.to_string()}) ThemedText },
        }
        PromptButton { answer: {answer} }
        on(|activate: On<Activate>,
            buttons: Query<&PromptButton>,
            mut asking: ResMut<Asking>,
            mut doc: ResMut<WorldDoc>,
            mut selection: ResMut<Selection>,
            mut status: ResMut<StatusMessage>,
            mut exit: MessageWriter<AppExit>| {
            let Ok(button) = buttons.get(activate.event_target()) else {
                return;
            };
            super::prompt::answer(
                button.answer,
                &mut asking,
                &mut doc,
                &mut selection,
                &mut status,
                &mut exit,
            );
        })
    }
}

impl Default for PromptRow {
    fn default() -> Self {
        Self {
            question: Question::UnsavedOnClose,
        }
    }
}

impl Default for PromptButton {
    fn default() -> Self {
        Self {
            answer: Answer::Cancel,
        }
    }
}
