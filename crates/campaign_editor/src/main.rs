//! The campaign editor: a window onto one campaign directory, the chrome for choosing
//! which one, the map of the terrain it names, and the features authored on top of it.
//!
//! A campaign named on the command line is opened before the window exists, so its
//! terrain load is process start rather than a frozen first frame. Every other way in
//! goes through the dialog, where there is a window to freeze, and so runs off the
//! render thread.
//!
//! The app owns its own exit. Bevy closes a window in `Last`, after every `Update` a
//! prompt could run in, so a close cannot be held back to ask about unsaved work unless
//! the window is built refusing to close itself — which means something must answer every
//! close request in every state, or the result is a build that cannot be quit.

use bevy::feathers::FeathersPlugins;
use bevy::feathers::dark_theme::create_dark_theme;
use bevy::feathers::theme::{ThemedText, UiTheme};
use bevy::input_focus::InputFocus;
use bevy::input_focus::tab_navigation::TabGroup;
use bevy::prelude::*;
use bevy::window::{ExitCondition, WindowCloseRequested};
use campaign::Campaign;

mod control;
mod dialog;
mod document;
mod features;
mod map;
mod notes;

/// The order the editor's two halves run in.
///
/// Sets rather than named systems: the map registers its own systems as one anonymous
/// chain, and an authoring system ordered `after(drive_camera)` across a plugin boundary
/// is an edge that rots the moment that chain is reordered.
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EditorSet {
    /// Loading the terrain, driving the camera, streaming the chunks.
    Map,
    /// Everything that draws, selects, reshapes and measures what the GM authored.
    Authoring,
}

/// The one line the tool says things on, wherever the message came from.
#[derive(Component, Default, Clone)]
pub struct StatusLine;

/// The dialog's root, so it can be taken off the screen without knowing what is under
/// it.
#[derive(Component, Default, Clone)]
pub struct DialogRoot;

/// The campaign on screen. Present only once one has been opened, so everything that
/// needs a campaign runs under `resource_exists::<OpenCampaign>` and nothing has to
/// ask whether there is one.
#[derive(Resource)]
pub struct OpenCampaign(pub Campaign);

/// Whatever the tool last had to say, from whichever part of it said it.
///
/// A resource rather than each system writing the line itself: the dialog, the map, the
/// document and the keys all have something to report, and four writers racing for one
/// `Text` means whichever the schedule happens to run last wins. One writer,
/// [`sync_status`], and one place to look for what went wrong.
#[derive(Resource, Default)]
pub struct StatusMessage(pub String);

impl StatusMessage {
    /// Say `message`, replacing whatever was there.
    pub fn say(&mut self, message: impl Into<String>) {
        self.0 = message.into();
    }

    /// Say what opening `campaign` left worth saying, or clear the line.
    pub fn opened(&mut self, campaign: &Campaign) {
        self.0 = if campaign.terrain_travels() {
            String::new()
        } else {
            format!(
                "terrain at {} is outside the campaign and will not travel with it",
                campaign.terrain_dir().display()
            )
        };
    }
}

fn main() {
    let mut app = App::new();
    app.add_plugins(DefaultPlugins.set(WindowPlugin {
        close_when_requested: false,
        exit_condition: ExitCondition::DontExit,
        ..default()
    }))
    .add_plugins(FeathersPlugins)
    .insert_resource(UiTheme(create_dark_theme()))
    .init_resource::<StatusMessage>()
    .add_plugins(control::ControlPlugin)
    .add_plugins(dialog::DialogPlugin)
    .add_plugins(map::MapPlugin)
    .add_plugins(features::FeaturesPlugin)
    .add_plugins(notes::NotesPlugin)
    .add_systems(Startup, (status_bar.spawn(), shell.spawn().run_if(no_campaign)))
    .add_systems(
        Update,
        (
            close_dialog.run_if(resource_added::<OpenCampaign>),
            sync_status.run_if(resource_changed::<StatusMessage>),
            close_without_a_document.run_if(not(resource_exists::<document::WorldDoc>)),
        ),
    );

    if let Some(path) = std::env::args().nth(1) {
        match Campaign::open(&path) {
            Ok(campaign) => {
                let mut status = StatusMessage::default();
                status.opened(&campaign);
                app.insert_resource(status);
                app.insert_resource(OpenCampaign(campaign));
            }
            Err(error) => {
                eprintln!("{path}: {error}");
                app.insert_resource(StatusMessage(error.to_string()));
                app.insert_resource(dialog::DialogFields {
                    root: path,
                    terrain: String::new(),
                });
            }
        }
    }

    app.run();
}

/// Copies [`StatusMessage`] onto the status line whenever it changes.
///
/// Writes only where the text actually differs, so an unchanged message does not mark
/// the `Text` changed and re-lay-out the line every frame.
pub fn sync_status(status: Res<StatusMessage>, mut lines: Query<&mut Text, With<StatusLine>>) {
    for mut text in lines.iter_mut() {
        if text.0 != status.0 {
            text.0 = status.0.clone();
        }
    }
}

fn close_without_a_document(
    mut requests: MessageReader<WindowCloseRequested>,
    mut exit: MessageWriter<AppExit>,
) {
    if requests.read().next().is_some() {
        exit.write(AppExit::Success);
    }
}

fn close_dialog(
    mut commands: Commands,
    mut focus: ResMut<InputFocus>,
    dialogs: Query<Entity, With<DialogRoot>>,
) {
    for entity in dialogs.iter() {
        commands.entity(entity).despawn();
    }
    focus.clear();
}

fn no_campaign(open: Option<Res<OpenCampaign>>) -> bool {
    open.is_none()
}

fn status_bar() -> impl Scene {
    bsn! {
        Node {
            position_type: PositionType::Absolute,
            bottom: px(10),
            left: px(12),
            right: px(12),
        }
        Pickable::IGNORE
        Children [ (Text("") ThemedText StatusLine Pickable::IGNORE) ]
    }
}

fn shell() -> impl Scene {
    bsn! {
        Node {
            position_type: PositionType::Absolute,
            width: percent(100),
            height: percent(100),
        }
        TabGroup
        DialogRoot
        Children [ dialog::dialog() ]
    }
}
