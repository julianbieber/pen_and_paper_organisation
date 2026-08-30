//! The campaign editor: a window onto one campaign directory, the chrome for choosing
//! which one, and the map of the terrain it names.
//!
//! A campaign named on the command line is opened before the window exists, so its
//! terrain load is process start rather than a frozen first frame. Every other way in
//! goes through the dialog, where there is a window to freeze, and so runs off the
//! render thread.

use bevy::feathers::FeathersPlugins;
use bevy::feathers::dark_theme::create_dark_theme;
use bevy::feathers::theme::{ThemedText, UiTheme};
use bevy::input_focus::InputFocus;
use bevy::input_focus::tab_navigation::TabGroup;
use bevy::prelude::*;
use campaign::Campaign;

mod dialog;
mod map;

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

fn main() {
    let mut app = App::new();
    app.add_plugins(DefaultPlugins)
        .add_plugins(FeathersPlugins)
        .insert_resource(UiTheme(create_dark_theme()))
        .add_plugins(dialog::DialogPlugin)
        .add_plugins(map::MapPlugin)
        .add_systems(Startup, (status_bar.spawn(), shell.spawn().run_if(no_campaign)))
        .add_systems(
            Update,
            close_dialog.run_if(resource_added::<OpenCampaign>),
        );

    if let Some(path) = std::env::args().nth(1) {
        match Campaign::open(&path) {
            Ok(campaign) => {
                app.insert_resource(OpenCampaign(campaign));
            }
            Err(error) => {
                eprintln!("{path}: {error}");
                app.insert_resource(dialog::DialogStatus(error.to_string()));
                app.insert_resource(dialog::DialogFields {
                    root: path,
                    terrain: String::new(),
                });
            }
        }
    }

    app.run();
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
        Children [ (Text("") ThemedText StatusLine) ]
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
