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
use campaign::brush::Brush;
use campaign::draft::DraftShape;
use campaign::feature::{FeatureKind, Rank};
use campaign::tiles::{CombatTile, DungeonTile};

use crate::combat::CombatMaps;
use crate::document::WorldDoc;
use crate::features::draw::Drafting;
use crate::CampaignChrome;

/// What the left button does on the map.
///
/// Three, because the three drawing tools differ in the shape they produce and in nothing
/// else — carrying that as a [`DraftShape`] rather than as three more variants keeps one
/// taxonomy of shapes instead of a second one that has to be kept in step. Painting is a
/// fourth thing the button can do, and it is a tool rather than a shape because it authors
/// the backdrop rather than a feature.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    Select,
    Draw,
    Paint,
    Image,
    Measure,
    Token,
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
    /// What a stroke does to the cells it covers.
    pub brush: Brush,
    /// The tile a stroke lays on a dungeon, which [`Brush::Room`] ignores.
    pub tile: DungeonTile,
    /// The tile a stroke lays on a combat map, which [`Brush::Room`] ignores.
    pub combat_tile: CombatTile,
}

impl Default for ActiveTool {
    fn default() -> Self {
        Self {
            tool: Tool::Select,
            shape: DraftShape::Point,
            point_kind: FeatureKind::Poi,
            polyline_kind: FeatureKind::Road,
            polygon_kind: FeatureKind::Territory,
            brush: Brush::Freehand,
            tile: DungeonTile::Floor,
            combat_tile: CombatTile::Dirt,
        }
    }
}

impl ActiveTool {
    /// Take a tool a combat map offers: the paint, select or token tool in hand is kept, and
    /// any other becomes the paint tool.
    pub fn enter_combat(&mut self) {
        if !matches!(self.tool, Tool::Paint | Tool::Select | Tool::Token) {
            self.tool = Tool::Paint;
        }
    }

    /// Put down the tools only a combat map offers, going back to a document that carries a
    /// grid when `onto_grid` holds: the token tool becomes the select tool, and the paint
    /// tool does too off a grid.
    pub fn leave_combat(&mut self, onto_grid: bool) {
        if self.tool == Tool::Token {
            self.tool = Tool::Select;
        }
        self.leave_a_grid_if(!onto_grid);
    }

    /// Whether the active tool places tokens.
    pub fn placing_tokens(&self) -> bool {
        self.tool == Tool::Token
    }

    /// Whether the active tool draws a feature.
    pub fn drawing(&self) -> bool {
        self.tool == Tool::Draw
    }

    /// Whether the active tool paints the backdrop.
    pub fn painting(&self) -> bool {
        self.tool == Tool::Paint
    }

    /// Whether the active tool places the imported picture.
    pub fn placing(&self) -> bool {
        self.tool == Tool::Image
    }

    /// Whether the active tool measures.
    pub fn measuring(&self) -> bool {
        self.tool == Tool::Measure
    }

    /// Whether the active tool selects.
    ///
    /// Stated rather than derived as "not drawing", which was true while there were two
    /// tools and silently made selection run under a brush stroke when there were three.
    pub fn selecting(&self) -> bool {
        self.tool == Tool::Select
    }

    /// Fall back to the select tool when `left` says a grid is no longer open.
    ///
    /// The paint tool and its rows are hidden off a grid, and a hidden tool that is still
    /// in hand is a left button that does nothing with no way to see why.
    pub fn leave_a_grid_if(&mut self, left: bool) {
        if left && self.painting() {
            self.tool = Tool::Select;
        }
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

/// A button that chooses what a stroke does to the cells it covers.
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct BrushButton {
    pub brush: Brush,
}

/// A button that chooses the tile a stroke lays.
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct TileButton {
    pub tile: DungeonTile,
}

/// A button that chooses the tile a stroke lays on a combat map.
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct CombatTileButton {
    pub tile: CombatTile,
}

/// A row shown only while what is on screen has a grid to paint.
#[derive(Component, Default, Debug, Clone, Copy, PartialEq)]
pub struct GridRow;

/// The row of tools that author a world document, hidden while a combat map is on screen.
#[derive(Component, Default, Debug, Clone, Copy, PartialEq)]
pub struct MapToolRow;

/// The tools a combat map offers besides painting, shown only while one is on screen.
#[derive(Component, Default, Debug, Clone, Copy, PartialEq)]
pub struct CombatToolRow;

/// The dungeon's tiles, shown only while a dungeon is on screen.
#[derive(Component, Default, Debug, Clone, Copy, PartialEq)]
pub struct DungeonTileRow;

/// The combat map's tiles, shown only while a combat map is on screen.
#[derive(Component, Default, Debug, Clone, Copy, PartialEq)]
pub struct CombatTileRow;

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

/// Marks which tool, kind, brush and tile are live, and shows only the rows that apply to
/// what is on screen: the map tools and kinds off a combat map, the select and token tools
/// on one, the brushes on any grid, and the tiles of the grid on screen.
///
/// The one system that touches the strip, and it only writes: nothing here reads a button
/// to find out what was pressed.
pub fn sync_tool_strip(
    active: Res<ActiveTool>,
    doc: Option<Res<WorldDoc>>,
    combat: Option<Res<CombatMaps>>,
    mut tools: Query<(&ToolButton, &mut ButtonVariant), (Without<KindButton>, Without<BrushButton>, Without<TileButton>, Without<CombatTileButton>)>,
    mut kinds: Query<(&KindButton, &mut ButtonVariant), (Without<ToolButton>, Without<BrushButton>, Without<TileButton>, Without<CombatTileButton>)>,
    mut brushes: Query<(&BrushButton, &mut ButtonVariant), (Without<ToolButton>, Without<KindButton>, Without<TileButton>, Without<CombatTileButton>)>,
    mut tiles: Query<(&TileButton, &mut ButtonVariant), (Without<ToolButton>, Without<KindButton>, Without<BrushButton>, Without<CombatTileButton>)>,
    mut combat_tiles: Query<(&CombatTileButton, &mut ButtonVariant), (Without<ToolButton>, Without<KindButton>, Without<BrushButton>, Without<TileButton>)>,
    mut rows: Query<
        (&mut Node, Option<&KindRow>, Has<GridRow>, Has<MapToolRow>, Has<CombatToolRow>, Has<DungeonTileRow>, Has<CombatTileRow>),
        Or<(With<KindRow>, With<GridRow>, With<MapToolRow>, With<CombatToolRow>, With<DungeonTileRow>, With<CombatTileRow>)>,
    >,
) {
    let in_combat = combat.is_some_and(|combat| combat.is_on_screen());
    let on_a_dungeon = !in_combat && doc.is_some_and(|doc| doc.document.world().grid().is_some());

    for (button, mut variant) in tools.iter_mut() {
        let live = button.tool == active.tool
            && (button.tool != Tool::Draw || button.shape == active.shape);
        set_variant(&mut variant, live);
    }
    for (button, mut variant) in kinds.iter_mut() {
        let live = active.kind_for(button.shape) == button.kind;
        set_variant(&mut variant, live);
    }
    for (button, mut variant) in brushes.iter_mut() {
        set_variant(&mut variant, button.brush == active.brush);
    }
    for (button, mut variant) in tiles.iter_mut() {
        set_variant(&mut variant, button.tile == active.tile);
    }
    for (button, mut variant) in combat_tiles.iter_mut() {
        set_variant(&mut variant, button.tile == active.combat_tile);
    }
    for (mut node, kind, grid, map_tools, combat_tools, dungeon_tiles, combat_tiles) in rows.iter_mut() {
        let shown = if let Some(row) = kind {
            !in_combat && active.drawing() && row.shape == active.shape
        } else if map_tools {
            !in_combat
        } else if combat_tools || combat_tiles {
            in_combat
        } else if dungeon_tiles {
            on_a_dungeon
        } else {
            grid && (in_combat || on_a_dungeon)
        };
        show(&mut node, shown);
    }
}

fn show(node: &mut Node, shown: bool) {
    let display = if shown { Display::Flex } else { Display::None };
    if node.display != display {
        node.display = display;
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
        CampaignChrome
        Children [
            (
                Node {
                    display: Display::Flex,
                    flex_direction: FlexDirection::Row,
                    column_gap: px(6),
                }
                MapToolRow
                Children [
                    tool_button("Select", Tool::Select, DraftShape::Point),
                    tool_button("Point", Tool::Draw, DraftShape::Point),
                    tool_button("Line", Tool::Draw, DraftShape::Polyline),
                    tool_button("Area", Tool::Draw, DraftShape::Polygon),
                    tool_button("Image", Tool::Image, DraftShape::Point),
                    tool_button("Measure", Tool::Measure, DraftShape::Point)
                ]
            ),
            (
                Node {
                    display: Display::None,
                    flex_direction: FlexDirection::Row,
                    column_gap: px(6),
                }
                CombatToolRow
                Children [
                    tool_button("Select", Tool::Select, DraftShape::Point),
                    tool_button("Token", Tool::Token, DraftShape::Point)
                ]
            ),
            (
                Node {
                    display: Display::None,
                    flex_direction: FlexDirection::Row,
                    column_gap: px(6),
                }
                GridRow
                Children [
                    tool_button("Paint", Tool::Paint, DraftShape::Point),
                    brush_button(Brush::Freehand),
                    brush_button(Brush::Rectangle),
                    brush_button(Brush::Flood),
                    brush_button(Brush::Room)
                ]
            ),
            (
                Node {
                    display: Display::None,
                    flex_direction: FlexDirection::Row,
                    flex_wrap: FlexWrap::Wrap,
                    column_gap: px(6),
                    row_gap: px(4),
                    max_width: px(420),
                }
                DungeonTileRow
                Children [
                    tile_button(DungeonTile::Floor),
                    tile_button(DungeonTile::Wall),
                    tile_button(DungeonTile::Door),
                    tile_button(DungeonTile::SecretDoor),
                    tile_button(DungeonTile::StairsUp),
                    tile_button(DungeonTile::StairsDown),
                    tile_button(DungeonTile::Water),
                    tile_button(DungeonTile::Rubble),
                    tile_button(DungeonTile::Empty)
                ]
            ),
            (
                Node {
                    display: Display::None,
                    flex_direction: FlexDirection::Row,
                    flex_wrap: FlexWrap::Wrap,
                    column_gap: px(6),
                    row_gap: px(4),
                    max_width: px(420),
                }
                CombatTileRow
                Children [
                    combat_tile_button(CombatTile::Grass),
                    combat_tile_button(CombatTile::Dirt),
                    combat_tile_button(CombatTile::Road),
                    combat_tile_button(CombatTile::Sand),
                    combat_tile_button(CombatTile::Mud),
                    combat_tile_button(CombatTile::ShallowWater),
                    combat_tile_button(CombatTile::DeepWater),
                    combat_tile_button(CombatTile::Tree),
                    combat_tile_button(CombatTile::Bush),
                    combat_tile_button(CombatTile::Boulder),
                    combat_tile_button(CombatTile::Wall),
                    combat_tile_button(CombatTile::Floor)
                ]
            ),
            kind_row(DraftShape::Point, POINT_KINDS),
            kind_row(DraftShape::Polyline, POLYLINE_KINDS),
            kind_row(DraftShape::Polygon, POLYGON_KINDS)
        ]
    }
}

fn brush_button(brush: Brush) -> impl Scene {
    bsn! {
        @FeathersButton {
            @caption: bsn! { Text({brush.label().to_string()}) ThemedText },
        }
        BrushButton { brush: {brush} }
        on(|activate: On<Activate>,
            buttons: Query<&BrushButton>,
            mut active: ResMut<ActiveTool>,
            mut stroking: ResMut<crate::features::paint::Stroking>,
            mut placing: ResMut<crate::features::image::Placing>| {
            let Ok(button) = buttons.get(activate.event_target()) else {
                return;
            };
            stroking.abandon();
            placing.cancel();
            active.brush = button.brush;
            active.tool = Tool::Paint;
        })
    }
}

fn tile_button(tile: DungeonTile) -> impl Scene {
    bsn! {
        @FeathersButton {
            @caption: bsn! { Text({tile.label().to_string()}) ThemedText },
        }
        TileButton { tile: {tile} }
        on(|activate: On<Activate>,
            buttons: Query<&TileButton>,
            mut active: ResMut<ActiveTool>,
            mut stroking: ResMut<crate::features::paint::Stroking>| {
            let Ok(button) = buttons.get(activate.event_target()) else {
                return;
            };
            stroking.abandon();
            active.tile = button.tile;
        })
    }
}

fn combat_tile_button(tile: CombatTile) -> impl Scene {
    bsn! {
        @FeathersButton {
            @caption: bsn! { Text({tile.label().to_string()}) ThemedText },
        }
        CombatTileButton { tile: {tile} }
        on(|activate: On<Activate>,
            buttons: Query<&CombatTileButton>,
            mut active: ResMut<ActiveTool>,
            mut stroking: ResMut<crate::features::paint::Stroking>| {
            let Ok(button) = buttons.get(activate.event_target()) else {
                return;
            };
            stroking.abandon();
            active.combat_tile = button.tile;
        })
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
            mut drafting: ResMut<Drafting>,
            mut stroking: ResMut<crate::features::paint::Stroking>,
            mut placing: ResMut<crate::features::image::Placing>,
            mut ruler: ResMut<crate::features::ruler::Ruler>,
            mut tokens: ResMut<crate::features::token::TokenGesture>| {
            let Ok(button) = buttons.get(activate.event_target()) else {
                return;
            };
            tokens.cancel_drag();
            drafting.abandon();
            stroking.abandon();
            placing.cancel();
            ruler.clear();
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

impl Default for BrushButton {
    fn default() -> Self {
        Self {
            brush: Brush::Freehand,
        }
    }
}

impl Default for CombatTileButton {
    fn default() -> Self {
        Self {
            tile: CombatTile::Grass,
        }
    }
}

impl Default for TileButton {
    fn default() -> Self {
        Self {
            tile: DungeonTile::Floor,
        }
    }
}
