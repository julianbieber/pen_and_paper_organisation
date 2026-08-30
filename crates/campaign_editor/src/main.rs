//! The campaign editor: a window onto one campaign directory, and the chrome for
//! choosing which one.
//!
//! A campaign named on the command line is opened before the window exists, so its
//! terrain load is process start rather than a frozen first frame. Every other way in
//! goes through the dialog, where there is a window to freeze, and so runs off the
//! render thread.

use bevy::feathers::FeathersPlugins;
use bevy::feathers::dark_theme::create_dark_theme;
use bevy::feathers::theme::UiTheme;
use bevy::input_focus::tab_navigation::TabGroup;
use bevy::prelude::*;
use campaign::Campaign;

mod dialog;

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
        .add_systems(Startup, (spawn_camera, shell.spawn()));

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

fn spawn_camera(mut commands: Commands) {
    commands.spawn(Camera2d);
}

fn shell() -> impl Scene {
    bsn! {
        Node {
            width: percent(100),
            height: percent(100),
            display: Display::Flex,
            flex_direction: FlexDirection::Column,
        }
        TabGroup
        Children [ dialog::dialog() ]
    }
}
