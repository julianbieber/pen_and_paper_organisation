//! Which tool is active, which kind each drawing tool will place, and the strip both are
//! read from.
//!
//! A feathers button has no state to poll — it is held or it is not, and being pressed is
//! an activation that happens once. So every button here writes what it means the moment
//! it is activated, through an observer, and no system reads a button at all. The one
//! system that does look at the strip writes *to* it, to show which entry is live.

use bevy::feathers::controls::{ButtonVariant, FeathersButton};
use bevy::feathers::theme::{ThemeBackgroundColor, ThemedText};
use bevy::feathers::tokens;
use bevy::prelude::*;
use bevy::ui_widgets::Activate;
use campaign::draft::DraftShape;
use campaign::feature::{FeatureKind, Rank};

use crate::features::draw::Drafting;

/// What the left button does on the map.
///
/// Only two, because the three drawing tools differ in the shape they produce and in
/// nothing else — carrying that as a [`DraftShape`] rather than as three more variants
/// keeps one taxonomy of shapes instead of a second one that has to be kept in step.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    Select,
    Draw,
}

/// The tool in hand, and what each drawing tool would place.
///
/// A kind per drawing tool rather than one shared kind: switching to the polyline tool to
/// draw a road and back again must not forget that points were being placed as taverns.
#[derive(Resource, Debug, Clone, Copy)]
pub struct ActiveTool {
    pub tool: Tool,
    pub shape: DraftShape,
    pub point_kind: FeatureKind,
    pub polyline_kind: FeatureKind,
    pub polygon_kind: FeatureKind,
}

impl Default for ActiveTool {
    fn default() -> Self {
        Self {
            tool: Tool::Select,
            shape: DraftShape::Point,
            point_kind: FeatureKind::Poi,
            polyline_kind: FeatureKind::Road,
            polygon_kind: FeatureKind::Territory,
        }
    }
}

impl ActiveTool {
    /// Whether the active tool draws rather than selects.
    pub fn drawing(&self) -> bool {
        self.tool == Tool::Draw
    }

    /// The kind the active shape would place.
    pub fn kind(&self) -> FeatureKind {
        self.kind_for(self.shape)
    }

    /// The kind `shape` would place.
    pub fn kind_for(&self, shape: DraftShape) -> FeatureKind {
        match shape {
            DraftShape::Point => self.point_kind,
            DraftShape::Polyline => self.polyline_kind,
            DraftShape::Polygon => self.polygon_kind,
        }
    }

    fn set_kind(&mut self, shape: DraftShape, kind: FeatureKind) {
        match shape {
            DraftShape::Point => self.point_kind = kind,
            DraftShape::Polyline => self.polyline_kind = kind,
            DraftShape::Polygon => self.polygon_kind = kind,
        }
    }
}

/// A button that chooses the tool, and the shape it draws.
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct ToolButton {
    pub tool: Tool,
    pub shape: DraftShape,
}

/// A button that chooses what one drawing tool places.
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct KindButton {
    pub shape: DraftShape,
    pub kind: FeatureKind,
}

/// The row of kinds belonging to one shape, shown only while that shape is active.
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct KindRow {
    pub shape: DraftShape,
}

/// What the point tool offers.
///
/// A settlement is here as well as under [`POLYGON_KINDS`] on purpose: a hamlet is
/// reasonably a point and a city is a polygon, and the model does not care which.
pub const POINT_KINDS: [FeatureKind; 3] = [
    FeatureKind::Poi,
    FeatureKind::Settlement,
    FeatureKind::DungeonEntry,
];

/// What the polyline tool offers. A settlement has no meaning as a line, so it is not
/// offered where it would produce nonsense.
pub const POLYLINE_KINDS: [FeatureKind; 3] =
    [FeatureKind::Road, FeatureKind::River, FeatureKind::Trail];

/// What the polygon tool offers. A road has no meaning as an area, and is not offered.
pub const POLYGON_KINDS: [FeatureKind; 3] = [
    FeatureKind::Territory,
    FeatureKind::Landcover,
    FeatureKind::Settlement,
];

/// The word for a kind in the strip and the property panel.
pub fn kind_label(kind: FeatureKind) -> &'static str {
    match kind {
        FeatureKind::Settlement => "settlement",
        FeatureKind::DungeonEntry => "dungeon",
        FeatureKind::Road => "road",
        FeatureKind::River => "river",
        FeatureKind::Trail => "trail",
        FeatureKind::Landcover => "landcover",
        FeatureKind::Territory => "territory",
        FeatureKind::Poi => "POI",
    }
}

/// The word for a settlement's rank in the property panel.
pub fn rank_label(rank: Rank) -> &'static str {
    match rank {
        Rank::Hamlet => "hamlet",
        Rank::Town => "town",
        Rank::City => "city",
    }
}

/// Hangs the tool strip over the map, once there is a document to author.
///
/// Choosing a tool or a kind abandons whatever the previous one was drawing: a half-drawn
/// polyline left standing while another tool places points is a shape no click can finish.
pub fn build_tool_strip(mut commands: Commands) {
    commands.spawn_scene(strip());
}

/// Marks which tool and which kind are live, and shows only the kinds the active shape
/// can place.
///
/// The one system that touches the strip, and it only writes: nothing here reads a button
/// to find out what was pressed.
pub fn sync_tool_strip(
    active: Res<ActiveTool>,
    mut tools: Query<(&ToolButton, &mut ButtonVariant), Without<KindButton>>,
    mut kinds: Query<(&KindButton, &mut ButtonVariant), Without<ToolButton>>,
    mut rows: Query<(&KindRow, &mut Node)>,
) {
    for (button, mut variant) in tools.iter_mut() {
        let live = button.tool == active.tool
            && (button.tool == Tool::Select || button.shape == active.shape);
        set_variant(&mut variant, live);
    }
    for (button, mut variant) in kinds.iter_mut() {
        let live = active.kind_for(button.shape) == button.kind;
        set_variant(&mut variant, live);
    }
    for (row, mut node) in rows.iter_mut() {
        let shown = active.drawing() && row.shape == active.shape;
        let display = if shown { Display::Flex } else { Display::None };
        if node.display != display {
            node.display = display;
        }
    }
}

fn set_variant(variant: &mut ButtonVariant, live: bool) {
    let wanted = if live {
        ButtonVariant::Primary
    } else {
        ButtonVariant::Normal
    };
    if *variant != wanted {
        *variant = wanted;
    }
}

fn strip() -> impl Scene {
    bsn! {
        Node {
            position_type: PositionType::Absolute,
            top: px(12),
            left: px(12),
            display: Display::Flex,
            flex_direction: FlexDirection::Column,
            row_gap: px(6),
            padding: px(8),
        }
        ThemeBackgroundColor(tokens::WINDOW_BG)
        Children [
            (
                Node {
                    display: Display::Flex,
                    flex_direction: FlexDirection::Row,
                    column_gap: px(6),
                }
                Children [
                    tool_button("Select", Tool::Select, DraftShape::Point),
                    tool_button("Point", Tool::Draw, DraftShape::Point),
                    tool_button("Line", Tool::Draw, DraftShape::Polyline),
                    tool_button("Area", Tool::Draw, DraftShape::Polygon)
                ]
            ),
            kind_row(DraftShape::Point, POINT_KINDS),
            kind_row(DraftShape::Polyline, POLYLINE_KINDS),
            kind_row(DraftShape::Polygon, POLYGON_KINDS)
        ]
    }
}

fn tool_button(caption: &'static str, tool: Tool, shape: DraftShape) -> impl Scene {
    bsn! {
        @FeathersButton {
            @caption: bsn! { Text({caption.to_string()}) ThemedText },
        }
        ToolButton { tool: {tool}, shape: {shape} }
        on(|activate: On<Activate>,
            buttons: Query<&ToolButton>,
            mut active: ResMut<ActiveTool>,
            mut drafting: ResMut<Drafting>| {
            let Ok(button) = buttons.get(activate.event_target()) else {
                return;
            };
            drafting.abandon();
            active.tool = button.tool;
            if button.tool == Tool::Draw {
                active.shape = button.shape;
            }
        })
    }
}

fn kind_row(shape: DraftShape, kinds: [FeatureKind; 3]) -> impl Scene {
    bsn! {
        Node {
            display: Display::None,
            flex_direction: FlexDirection::Row,
            column_gap: px(6),
        }
        KindRow { shape: {shape} }
        Children [
            kind_button(shape, kinds[0]),
            kind_button(shape, kinds[1]),
            kind_button(shape, kinds[2])
        ]
    }
}

fn kind_button(shape: DraftShape, kind: FeatureKind) -> impl Scene {
    bsn! {
        @FeathersButton {
            @caption: bsn! { Text({kind_label(kind).to_string()}) ThemedText },
        }
        KindButton { shape: {shape}, kind: {kind} }
        on(|activate: On<Activate>,
            buttons: Query<&KindButton>,
            mut active: ResMut<ActiveTool>,
            mut drafting: ResMut<Drafting>| {
            let Ok(button) = buttons.get(activate.event_target()) else {
                return;
            };
            drafting.abandon();
            active.set_kind(button.shape, button.kind);
        })
    }
}

impl Default for ToolButton {
    fn default() -> Self {
        Self {
            tool: Tool::Select,
            shape: DraftShape::Point,
        }
    }
}

impl Default for KindButton {
    fn default() -> Self {
        Self {
            shape: DraftShape::Point,
            kind: FeatureKind::Poi,
        }
    }
}

impl Default for KindRow {
    fn default() -> Self {
        Self {
            shape: DraftShape::Point,
        }
    }
}
