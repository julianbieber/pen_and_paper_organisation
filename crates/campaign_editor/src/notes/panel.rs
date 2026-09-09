//! The bottom-right panel: the notes no feature holds — a person, a faction, a session —
//! and, beneath them, what references the selected place.
//!
//! It hangs off the campaign rather than off the world document, because none of those
//! three has geometry — a `world.ron` that failed to load must not also take away the
//! notes that never needed one. The references section is selection-driven and so has
//! nothing to say without a document, which it says rather than disappearing.
//!
//! The section's own rules live in [`references`](crate::notes::references); this module
//! only hangs it under the buttons, so the panel has one root and one place on screen.
//!
//! It sits bottom-right, which is the one corner nothing else claims: the tool strip is
//! top-left, the property panel top-right, the river slider bottom-left and the status line
//! along the bottom. A panel over the middle of the map would swallow presses meant for the
//! terrain under it, which is what moving it here avoids.
//!
//! A button that cannot be pressed is disabled and says why, rather than hidden. Bevy's
//! tab navigation looks at neither `Node.display` nor `Visibility`, so a button hidden
//! that way is still in the tab ring and still answers Enter; `InteractionDisabled` is
//! what actually stops a press, and `bevy_feathers` already greys what carries it.

use bevy::feathers::controls::{FeathersButton, FeathersTextInput, FeathersTextInputContainer};
use bevy::feathers::theme::{ThemeBackgroundColor, ThemedText};
use bevy::feathers::tokens;
use bevy::prelude::*;
use bevy::text::{EditableText, TextEditChange};
use bevy::ui::InteractionDisabled;
use bevy::ui_widgets::Activate;
use campaign::notebook::NoteKind;

use crate::notes::{NoteJob, NoteTitle, ZkState};
use crate::{OpenCampaign, StatusMessage};

/// The notes panel's root.
#[derive(Component, Default, Clone)]
pub struct NotesPanelRoot;

/// The field a standalone note's title is typed into.
#[derive(Component, Default, Clone)]
pub struct NoteTitleField;

/// The line saying why the notes panel's buttons cannot be pressed.
///
/// Its own line rather than one shared with the property panel's, so neither system
/// overwrites what the other wrote.
#[derive(Component, Default, Clone)]
pub struct NewNoteReasonLine;

/// A button that makes one note of a kind that has no geometry.
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct NewNoteButton {
    pub kind: NoteKind,
}

/// Hangs the notes panel over the map.
pub fn build_notes_panel(mut commands: Commands) {
    commands.spawn_scene(panel());
}

/// Disables the notes panel's buttons when a note cannot be made, and says why.
///
/// Writes only where the answer differs from what the button already carries: adding
/// `InteractionDisabled` blindly every run is archetype churn on the buttons, and
/// `bevy_feathers` restyles on `Added`/`RemovedComponents`, so a blind re-insert also
/// re-runs the styling for nothing.
pub fn show_new_note_buttons(
    mut commands: Commands,
    job: Res<NoteJob>,
    zk: Res<ZkState>,
    title: Res<NoteTitle>,
    buttons: Query<(Entity, Has<InteractionDisabled>), With<NewNoteButton>>,
    mut lines: Query<&mut Text, With<NewNoteReasonLine>>,
) {
    let refusal = crate::notes::refusal(&job, &zk, &title.0);
    let wanted = refusal.is_some();

    for (entity, disabled) in buttons.iter() {
        if disabled == wanted {
            continue;
        }
        if wanted {
            commands.entity(entity).insert(InteractionDisabled);
        } else {
            commands.entity(entity).remove::<InteractionDisabled>();
        }
    }

    let shown = refusal.unwrap_or_default();
    for mut text in lines.iter_mut() {
        if text.0 != shown {
            text.0.clone_from(&shown);
        }
    }
}

fn panel() -> impl Scene {
    let kinds = NoteKind::without_geometry();
    bsn! {
        Node {
            position_type: PositionType::Absolute,
            bottom: px(44),
            right: px(12),
            width: px(320),
            display: Display::Flex,
            flex_direction: FlexDirection::Column,
            row_gap: px(6),
            padding: px(8),
        }
        ThemeBackgroundColor(tokens::WINDOW_BG)
        NotesPanelRoot
        Children [
            (Text("Notes") ThemedText),
            (
                @FeathersTextInputContainer
                Node { width: percent(100) }
                Children [
                    (
                        @FeathersTextInput
                        NoteTitleField
                        on(|change: On<TextEditChange>,
                            texts: Query<&EditableText>,
                            mut title: ResMut<NoteTitle>| {
                            if let Ok(text) = texts.get(change.event_target()) {
                                title.0 = text.value().to_string();
                            }
                        })
                    )
                ]
            ),
            (
                Node {
                    display: Display::Flex,
                    flex_direction: FlexDirection::Row,
                    flex_wrap: FlexWrap::Wrap,
                    column_gap: px(4),
                    row_gap: px(4),
                }
                Children [
                    new_note_button(kinds[0]),
                    new_note_button(kinds[1]),
                    new_note_button(kinds[2])
                ]
            ),
            (Text("") ThemedText NewNoteReasonLine),
            crate::notes::references::section()
        ]
    }
}

fn new_note_button(kind: NoteKind) -> impl Scene {
    let caption = format!("New {}", kind.label());
    bsn! {
        @FeathersButton {
            @caption: bsn! { Text({caption}) ThemedText },
        }
        NewNoteButton { kind: {kind} }
        InteractionDisabled
        on(|activate: On<Activate>,
            buttons: Query<&NewNoteButton>,
            title: Res<NoteTitle>,
            campaign: Res<OpenCampaign>,
            zk: Res<ZkState>,
            doc: Res<crate::document::WorldDoc>,
            mut job: ResMut<NoteJob>,
            mut status: ResMut<StatusMessage>| {
            let Ok(button) = buttons.get(activate.event_target()) else {
                return;
            };
            crate::notes::start(
                &mut job,
                &zk,
                &campaign,
                doc.path.clone(),
                &mut status,
                button.kind,
                &title.0,
                None,
            );
        })
    }
}

impl Default for NewNoteButton {
    fn default() -> Self {
        Self {
            kind: NoteKind::Person,
        }
    }
}
