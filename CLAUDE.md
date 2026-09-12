# pen_and_paper_organisation

A tool for running a pen-and-paper campaign. A top-down world map whose relief comes from
a baked [`watershed`](https://github.com/julianbieber/watershed) terrain; every road,
river, settlement, region and dungeon authored here; every place, person and faction
backed by a `zk` note whose backlinks the tool can show.

The map answers *where*, the notes answer *what*, and the point of the project is the
link between them: select Riverford, see the session where the party arrived, the NPC who
lives there and the faction that runs the docks.

## State

The workspace and the campaign directory are in (#1): `crates/campaign` opens and creates
one, `crates/campaign_editor` builds the `pnp` binary and shows a dialog when it has no
campaign. The terrain draws as a tile map (#2): `campaign::tiles` decides every tile,
`campaign_editor/src/map` streams them. Features are styled by kind and a city's interior
opens up as the map zooms in (#5): `campaign::style`, `campaign::lod` and `campaign::label`
decide it, `campaign_editor/src/features/render.rs` draws it. Notes are made from templates
and linked to features (#6): `campaign::notebook` owns the whole `zk` dependency,
`campaign_editor/src/notes` runs it off the frame. Selecting a place shows what references
it (#7): the same module builds the tag and the `zk list`, `notes/references.rs` caches an
answer per tag behind one query at a time, and `notes/watch.rs` drops the cache when the
notebook changes underneath us. Distances are measured at campaign scale (#10):
`campaign::measure` says what one cell is worth and rounds every figure, `map/scalebar.rs`
draws the bar, `map/scale.rs` is where the GM sets the scale and the party's pace, and
`features/ruler.rs` owns the measure tool. Dungeons are authored on a square tile grid (#8):
`campaign::grid` and `campaign::brush` own the grid and the four brushes,
`campaign_editor/src/document.rs` holds the open document and the parked ones, and
`map/backdrop.rs` is the one thing that says what is being drawn on. A document may also be
drawn over an imported picture (#9): `campaign::image` owns the placement arithmetic, the
refusals and the import, `campaign_editor/src/map/image.rs` reads and draws it, and
`features/image.rs` owns the gesture and the panel. The milestone is filed as issues #1–#11; the design
behind them is in the plan at
`~/fun_repos/hobby-mimisbrunnr/notes/pen_and_paper_organisation/init/pen_and_paper_plan/plan-2026-08-30-campaign-map-and-notes.md`.

**Campaign management is the next milestone, filed as #24–#31** (2026-09-12): the campaign
becomes one self-contained, git-backed directory. The terrain is copied in on create (#24):
`Campaign::create` in `crates/campaign/src/campaign.rs` takes the terrain as a source and
copies it into `<root>/terrain`, `crates/campaign/src/layout.rs` fixes that path and
`crates/campaign/src/manifest.rs` says whether an opened campaign's terrain travels with it;
`crates/campaign_editor`'s dialog and status line report it. A created campaign is a git
repository from birth (#25): `crates/campaign/src/repo.rs` is the one place this workspace
runs `git`, `Campaign::create` runs it after the manifest is written, and a missing or
refusing `git` is reported through `Created::repository` rather than refusing the campaign.
Remaining: a recent list (#26), create-by-name (#27), a folder picker (#28),
close-and-switch (#29), sync from the editor (#30) and clone from the dialog (#31). Order is
dependency order; the plan, with the two decisions it makes, is at
`~/fun_repos/hobby-mimisbrunnr/notes/pen_and_paper_organisation/planning/campaign_management/plan-2026-09-12-campaign-directory-and-sync.md`.

`gh issue view <n>` carries the scope and acceptance criteria for a session's work. Each
issue names what it depends on. After #1 there are three tracks that do not block each
other: terrain (#2, #10), model and editor (#3, #4, #5, #8, #9), notes (#6, #7).

**Update this file when a decision here stops being true.** It is what the next session
reads instead of re-deriving.

## The four rules everything else follows from

**Terrain gives landform and water. Everything else is a GM decision.** The imported
terrain is height plus a water solve, and that is all it is relied on for. A forest, a
marsh, a border, a road, a city is something the GM drew — a feature, not a field the
terrain happened to carry. So the backdrop is deliberately neutral: no biome tinting, no
colour derived from moisture or temperature, nothing that would fight the regions painted
on top of it.

`watershed` also cannot re-bake from a loaded `Terrain` by design, so `terrain/` is
immutable here and the authored content lives in separate files beside it. Nothing in this
repo bakes a terrain or reaches for `watershed_editor`. `Campaign::create` (#24) copies a
terrain export in byte for byte so the campaign directory can travel, but nothing writes
into it afterwards — the copy and the immutability are separate rules. Two files, two
lifetimes, nothing to merge.

**One feature type, in every document.** A feature is a `Point`, `Polyline` or `Polygon`
with a kind, a label, an optional linked note and an optional `parent`. Roads, rivers,
settlements, regions, dungeon entries, taverns and altars are all the same type with
different kinds. Resist adding a second one.

**A feature and a note are two halves of one thing.** The feature holds the note's path;
the note holds a `#place/<slug>` tag. "What references this" is then one `zk list --tag`,
and the identical query answers it for factions and persons — which have notes and no
geometry, and therefore no other way to be found.

**Every change is an `Edit` value.** A value the UI constructs and the control socket
parses, both taking the same `apply`, which returns its own inverse. Undo/redo and
headless testing fall out of this; they cannot be retrofitted onto direct mutation. Copied
from `~/fun_repos/watershed/crates/watershed_editor/src/edit.rs` — read it before
extending `Edit`.

## What one cell is worth

**`campaign::measure` answers it, and every figure a GM reads comes from there** (#10):
the scale bar, the ruler, the selection panel's length line and image calibration are all
handed the same `CellWorth`, so a distance typed into a calibration and one measured with
the ruler cannot disagree. `campaign_editor/src/map/scalebar.rs` owns a node's width,
`features/ruler.rs` owns the clicks, and neither chooses a figure.

**A document carrying a `TileGrid` is measured in feet off that grid**, whatever the
campaign's own unit is, and a cell of a dungeon this build creates is exactly five feet
(`DEFAULT_METRES_PER_CELL` is 1.524). Every other document uses `campaign.ron`'s
`units_per_cell` and `unit` — which before #10 were read by nothing at all, so the world
map silently measured one unit per cell.

**A dungeon carries no travel time.** A pace in days says nothing about a corridor, so
`CellWorth::travelled` is false there and every duration is absent rather than absurd.
That means changing `campaign.ron`'s unit does *not* change a figure inside a dungeon,
which amends #10's fourth acceptance criterion.

**The bar changes unit as the map zooms.** A figure below one steps down to a smaller unit
of the same family, so a kilometre campaign zoomed into a city reads in metres. That needs
`DistanceUnit` to carry a factor and a family, so `unit` is no longer a label nothing
branches on — but it stays a `String` on disk, and one this build does not know keeps its
own label at every zoom rather than being refused.

**`units_per_cell` is bounded above and below.** The scale bar is the first thing that
divides by it, and over the range `check` used to admit the round-figure search either did
not terminate or answered zero.

**A measurement is session state.** It becomes no `Edit`, reaches no document and is never
saved; leaving the tool, switching document or pressing Escape forgets it. It is therefore
cleared wherever every other in-flight gesture is — the tool button, `Command::Tool` and
`switch_document` — rather than defending itself.

## Cities are on the world map

A settlement is a polygon on the world map, and the tavern inside it is a point whose
`parent` is that polygon. It is drawn once the parent covers enough screen area, so world
zoom stays readable without a child document or a view switch. **Dungeons** are child
documents, because a 30-metre room has no meaningful position at terrain-cell scale.

## A document carries its own backdrop

**Whether a `World` holds a `TileGrid` is what decides which backdrop it is drawn on.** The
world map holds none and is drawn on the campaign's terrain; a dungeon holds one and is
drawn on it. One type, one `Edit` vocabulary, one undo stack — which is why the grid went
onto `World` rather than into a second document type that would have forked `Edit`,
`Document`, `WorldDoc` and the control socket with it.

A dungeon entry names its dungeon in `Feature::dungeon`, a **single file name** inside
`dungeons/`. It is derived from the entry's label through `campaign::slug`, and it is the
one place this workspace turns typed text into a filename — so it is guarded by
`dungeon_name_refusal`, which is stricter than `note_path_refusal` (no separator, no `..`,
a `.ron` extension) and is enforced in `World::check` *and* `Edit::apply`, as every rule
here is. Names are made unique against the paths the document already holds **and** the
directory: a dungeon that has been opened and not saved has a name and no file.

A dungeon does not open another. Nesting is refused where it is stored, so the rule is
checkable without a window and there is always exactly one place to come back to.

**Both documents stay live.** `campaign_editor/src/document.rs` parks whichever document is
not on screen, with its undo stack, its dirty flag and where the camera was looking at it,
so a round trip loses neither work nor history. The close guard therefore asks about every
document, not the one on screen. Feature ids are per-document, so a switch clears
everything holding one — the selection, a drag, a draft, a stroke, the pending label, a
question, and the document a note job was started against.

**`Backdrop` is the only source of a `MapView`.** Nine places used to build one out of the
terrain's extent; a consumer now asks the resource. It also carries where the camera goes,
because "restore or frame" is one question and only `place_camera` may answer it — a system
in the authoring set writing the camera would be overwritten by the map set on the next
frame. Its `generation` is what says the camera has not been placed for this backdrop yet.

**A brush stroke is one `Edit::PaintTiles`**, applied on release, carrying only the cells
that actually change and each of them once. That is what makes it a single press of undo
however many cells it covers, and what makes its inverse exact. The four brushes and the
tile vocabulary live in `campaign` and are tested without a GPU; `features/paint.rs` owns
only the gesture.

Tiles are written **run-length encoded**: one variant name per cell would put a middling
grid over `MAX_WORLD_BYTES`, and a document that paints happily and can never be saved is
the worst failure available. A 64-cell grid with a room in it is about a kilobyte.

## A picture can sit under the features

**An imported image is an overlay in document cells, not a third `BackdropSource`.** The
backdrop enum is exclusive and drives `stream_chunks`, which fills tile *indices*; a picture
has none, and the issue's own case is a scan inside a dungeon, which must still carry the
grid that makes it one. So a document may hold a `TileGrid`, an `ImageBackdrop`, both or
neither, and the picture is positioned in the same cells everything else is.

**A document names its picture, it does not path it.** One file name inside `images/`,
joined onto the campaign root by `layout::image` and nowhere else, which is what lets the
whole directory move. That makes the name a trust boundary, so it is derived through
`campaign::slug` and stored only after `image::name_refusal` — which is
`dungeon_name_refusal`'s rule, shared as `feature::file_name_refusal` rather than copied a
fifth time, and it additionally refuses `:` and `#`, because those name an asset source and
a label rather than a file.

**The picture is not asked of the asset server, deliberately.** The asset root is fixed when
the app builds and a campaign is chosen at runtime, so the server cannot address one; the
ways around that are worse, since the path would come from a `world.ron` somebody else may
have written, and a `.meta` sidecar beside the picture selects an arbitrary loader by name.
`map/image.rs` reads the bytes on the IO pool and decodes them itself, which also makes
missing, unreadable and not-a-picture three sentences instead of one opaque load failure.
The import refuses anything that is not a regular file, anything over `MAX_IMAGE_BYTES` or
`MAX_IMAGE_SIDE`, and decides the format from the header rather than the name — then takes
its destination with `File::create_new`, so uniquing the name and claiming it are one
atomic step and a symlink is refused rather than written through.

**Three depths, two mechanisms.** `TERRAIN_Z < IMAGE_Z < FEATURE_Z` in `map/view.rs`, with a
compile-time assertion. The picture covers the chunks *because its z is greater*; a feature
covers the picture for an unrelated reason — a feature is a gizmo, every gizmo is queued at
one depth with the comparison always passing, so it draws last at any z. The dungeon's grid
lines are gizmos too, so they are drawn **over** a scan, which is what registering one
against a grid wants.

**Every gesture lands one `Edit` on release** — a drag, a corner scale, a calibration and an
opacity drag alike. The opacity slider is the sharp case: it moves every frame it is
dragged, the undo stack holds 128 entries and applying an edit clears the redo stack, so an
edit per frame would silently discard everything the GM drew. `show_image_panel` writes the
slider back **only on the frame the document changed**; writing whenever the two differ is a
loop with the thing that reads it, and the value could never reach the document at all.

**Calibration is measured in the document's own unit** — the grid's `metres_per_cell` in a
dungeon, the manifest's `units_per_cell` on the world map. The two differ by a factor of
1500 at the defaults, so `campaign::image::calibrate` takes it as an argument rather than
assuming; the ruler (#10) must read the same answer. The first of the two marks is what
stays put across the rescale.

## Layout

```
crates/campaign          model, serde, io, zk client — NO bevy, tested without a GPU
crates/campaign_editor   the bevy app, binary `pnp`

my-campaign/
├── campaign.ron         version, name, terrain dir, units_per_cell, unit
├── .gitignore           notes/.zk/notebook.db* and *.tmp, written on create (#25)
├── terrain/             watershed output, copied in on create (#24), read-only after
├── world.ron            features
├── dungeons/*.ron       child documents
├── images/              imported backdrops, copied in so the directory stays portable
└── notes/               zk notebook; .zk/templates/ holds place, person, faction, session
```

The crate split is the one `watershed` uses and exists for the same reason: the model is
testable without a window. **Anything that can live in `campaign` does.** A rule that
needs a GPU to test is a rule that will not be tested.

**A campaign `Campaign::create` builds always has its terrain inside it** (#24): `create`
takes the terrain as a source and copies the whole export into `<root>/terrain/`, then
writes `terrain: "terrain"`, so the directory it returns can be moved, copied or cloned
and still open. `open` still honours a manifest naming a terrain elsewhere — a hand
edit, or one written by an earlier build — and `terrain_travels()` says whether it will
move with the directory; the status line warns when it will not. `world.ron` sitting
inside the campaign directory stays a convention `Campaign::create` does not write —
that belongs to #3. **One terrain per campaign**: `campaign.ron` has a single `terrain`
field, and `open` loads it eagerly, so a second would be a second load.

**A `Campaign`'s root and terrain are fixed at `open`; its scale is the one thing that
changes.** It holds what is on disk — root, manifest, terrain — and the undo stack, the
dirty flag, the notebook handle and any tile cache are editor-session state and never
fields of it, so #3's mutable world document is its own type rather than a
`ResMut<Campaign>` serialising against #2's terrain reads every frame. `Campaign::rescale`
(#10) is the sole mutation, and it keeps the rule's point intact: it writes `campaign.ron`
first and re-reads the manifest after, so the value still says only what is on disk. It
deliberately does **not** re-open — a scale is a few bytes of manifest, and `open` would
read the whole terrain again on the thread drawing the window.

**An absent `world.ron` is an empty world, not an error** — `create` writes none, so #3
reads "missing" as "default" rather than inventing a migration.

**Nothing in this repo ever calls `Terrain::save_to_dir`.** It deletes `layer_<n>.png`
files it does not name, and terrain here is read-only.

`create` still makes a plain `notes/` directory. `campaign::notebook` makes it a notebook
the first time a note is asked for (#6): `zk init` only when there is no `.zk`, then the
four templates ensured on every create, each written only where no file of that name is
there — so a notebook the GM made themselves gains them, one whose templates were
half-written repairs itself, and a template the GM has edited is never overwritten. The
subprocess `create` itself runs is git's, not `zk`'s — see below.

**A created campaign is a git repository from birth** (#25): `campaign::repo` is the one
place this workspace runs `git`, mirroring `campaign::notebook`'s shape for `zk`. After the
manifest is written, `Campaign::create` runs `Repo::at(root).begin(git)` — write
`.gitignore` (`create_new`, so a GM's own is never overwritten), `init` unless the root is
already inside a work tree, `add`, `commit` — and keeps the outcome in `Created::repository`
rather than propagating it: a missing or refusing `git` costs the GM the repository, never
the campaign. The status line reports it through `StatusMessage::created`. A writer creates
the directory it writes into — `World::save` does, before opening its temporary — because
git does not carry an empty one, and a clone that never held a dungeon must still be able to
save one.

**`crates/campaign` still passes its tests with nothing on `PATH`.** The argument vectors
are pure functions tested against a recorded `Runner`/`GitRunner`; the handful of tests
wanting the real programs live in `tests/notebook_zk.rs` and `tests/repo_git.rs` and skip
themselves, with `PNP_REQUIRE_ZK=1` and `PNP_REQUIRE_GIT=1` turning the skip into a failure
so CI cannot pass having checked none of it.

## Conventions

- **Coordinates are backdrop cells** everywhere in a map document, so a feature and the
  cell under it agree without a conversion at every call site. On the world map that is a
  terrain cell and `campaign.ron` holds `units_per_cell` for anything the GM reads; in a
  dungeon it is a grid cell and the grid holds its own `metres_per_cell`. The thresholds in
  `lod` and the widths in `style` are stated in cells per logical pixel either way, so they
  mean different ground in a dungeon than on the world map — which is the point of a child
  document having its own scale.
- **Bevy 0.19. All UI is `bevy_feathers`** — the toolkit `watershed_editor` uses, so its
  chrome and idioms carry over. No `egui`.
- `watershed` comes in by **git URL with a pinned `rev`**, never a path — the repo has to
  build without a sibling checkout. Local watershed work goes in `.cargo/config.toml`,
  which is gitignored, as it is in `watershed` itself. It cannot go in the root
  `Cargo.toml`: that file is tracked, so "uncommitted" would mean leaving it permanently
  dirty.
  ```toml
  # .cargo/config.toml — never committed
  [patch."https://github.com/julianbieber/watershed"]
  watershed = { path = "../watershed/crates/watershed" }
  ```
  An active patch rewrites `Cargo.lock`, which **is** committed — check `git diff
  Cargo.lock` before committing while one is in place.
- A `zk` invocation is a subprocess: run it on the async task pool and land the result,
  as `watershed_editor/src/document.rs` lands its jobs. The map must not stutter because
  a query is in flight.
- `zk` may be absent. Report it clearly and leave the map working — the map does not
  depend on the notebook.

## The watershed read API

`Terrain::load_from_dir` is the only way to obtain one. What a consumer reads:

| | |
|---|---|
| `terrain.width() / height()` | extent in cells |
| `terrain.fields()` | `FieldView` per field: `name`, `role`, `range_low/high`, `is_categorical` |
| `terrain.field(name)` / `field_with_role(role)` | both `Option` |
| `FieldView::value_at(x, y)` / `sample(x, y)` | a cell, or interpolated; categorical fields snap to nearest |
| `terrain.water()` → `Option<WaterView>` | `is_water`, `depth_at`, `accumulation`, `channel_at(x, y, threshold)`, `flow_at`, `lakes` |

**Only `Height` and the water solve are read.** `FieldRole` is `Height`, `Moisture`,
`Custom` — there is no biome or temperature role, and the crate states a `Custom` field
never resolves by role. A terrain may carry such fields; this tool does not colour by
them, because that is the GM's decision to draw (see the first rule above).

Everything is fallible. `water()` is `None` on a terrain with no solve, every lookup
returns `Option`, and **a terrain with only a height field and no water must still
render.** A terrain with no height field is the one case worth refusing outright.

Rivers are `accumulation` above a threshold — that threshold is the one control deciding
whether the map reads as a drainage basin or a puddle, so it is adjustable at runtime.
Compare with `>`, never `WaterView::channel_at`, which is `>=`: `accumulation` answers
`0.0` off the edge of the terrain, so `>=` at a threshold of zero makes everything a river.

Worked example: `~/fun_repos/watershed/crates/watershed/examples/load_terrain.rs`.

## How the terrain is drawn

**Hand-drawn pixel-art tiles, not a procedural ramp.** Height picks one of `LAND_BANDS`
tiles, the water solve picks water, river and coastline tiles, and relief comes from a
per-tile hillshade **tint** rather than from tiles of its own — so slope costs no art.

The renderer is bevy's own `bevy_sprite_render::tilemap_chunk`: one `TilemapChunk` entity
per `CHUNK_CELLS`-square block of terrain cells, one draw call each, streamed so only what
the camera can see is resident.

**The dungeon strip is generated, not drawn.** `map/load.rs` builds one flat-coloured layer
per `DungeonTile` from the table in `campaign::style`, stacked vertically because that is
the layout `reinterpret_stacked_2d_as_array` splits — the terrain's horizontal strip is
turned into an array by the image loader instead, which is a different route to the same
array texture. A tile's layer is its `DungeonTile::index` either way. The four door and
stair kinds therefore differ by hue alone; that is deliberate, and replacing
`build_dungeon_tileset` with a hand-drawn strip later changes nothing else.

**The terrain tileset is drawn by hand in `bevy_sprite_editor`** (`~/fun_repos/bevy_sprite_editor`)
and lives at `crates/campaign_editor/assets/terrain_tiles.png` with its
`.atlas.json` sidecar. That tool only ever grows an atlas rightwards, so the file is a
**single row** of square tiles, row-major over the whole image. Bevy loads that strip
straight into the array texture the tilemap material wants, via
`ImageArrayLayout::GridCount { columns, rows: 1 }` — which walks tiles row-major, so
**a tile's column is its array layer is its index**. Nothing here ever writes either file.

The strip's columns are listed on `TileKind` in `crates/campaign/src/tiles.rs`, which is
also the only place a tile number is written. **A redraw that reorders the strip renders
happily and wrongly**, so the order is the contract: 6 land bands low to high, shallow
water, deep water, river, then 15 coastline tiles indexed by which of a cell's four
neighbours are water (N, E, S, W from the low bit). There is no tile for "no wet
neighbour" — that cell is a band, not a coast — so the coastline tiles start at mask 1
and `TILE_COUNT` is 24, not 25.

**Every decision lives in `crates/campaign`** — which tile, which cells a chunk covers,
where the terrain's rows land — and is tested without a GPU. `crates/campaign_editor/map`
owns only ECS and pixels. A terrain's rows run top to bottom and a tilemap chunk's run
bottom to top; that flip is written once, in `map::view`, and everything goes through it.

## How features are styled

**Three tables in `crates/campaign`, and the renderer holds no opinion of its own.**
`style::of(kind, rank)` gives every colour, width, dash, fill and icon; `lod::detail`
decides how much of a feature is drawn; `label::place` decides which labels fit. All three
are pure functions over a `World`, so every rule below is tested without a GPU and
`features/render.rs` owns only ECS and pixels.

**A `Stroke` names a pen, it does not carry a width.** A gizmo's width and dash live in a
configuration group, which is a *type*, so the set of pens is fixed at compile time. A new
`Stroke` fails to compile twice — in `style.rs`, which must give it a width and a dash, and
in `features/pens.rs`, which must give it a group and a place in the paint order.

**Registration order is paint order.** Bevy queues every 2D gizmo at one depth with the
comparison always passing, so z orders a line against the *terrain* and against nothing
else. What covers what is the sequence in `register_pens`: fills, feature strokes, labels,
the draft, the handles.

**One currency: cells per _logical_ pixel.** Every threshold, slack and stored reveal scale
is logical, so a document authored on one display behaves identically on another. The sole
conversion to physical pixels is a pen's width in `pens::size_pens`, because that is what
the line shader measures against. `OrthographicProjection::scale` is world units to the
*logical* pixel — bevy feeds `ScalingMode::WindowSize` the logical viewport — which is why
`viewport_of` returns `logical_viewport_size`.

**Two thresholds, both fades, both decided in `lod::detail`.** A feature's own
`max_cells_per_pixel`, and the on-screen area of every polygon in its parent chain. The one
answer drives the skip, the alpha and the label fade together, and the same function
filters every hit test — so a press can never land on something the frame declined to draw.
A selected feature is exempt inside that function, or it could not be clicked to deselect.
Areas come from `lod::Areas`, cached beside the document on `WorldDoc` and rebuilt wherever
an edit lands.

**A dash restarts at every vertex.** The line shader measures a dash along one segment, so
a road authored at cell density renders solid. `render::segments_of` coalesces segments
shorter than one dash period before stroking. The `River` pen is the one that may not be
dashed at all, because its width is rewritten from the zoom every frame to match the
terrain's own one-cell channel.

**Labels are automatic only.** `label::anchor` places them — a point above its icon, a
polygon at an interior point, a polyline at its arc-length midpoint — and there is no
per-feature offset. The format stays open to adding one. Boxes are measured by the renderer
and over-estimated, so an error drops a label rather than letting two overlap; ties break
on `FeatureId`, never on document order. Only ASCII 32–126 draws, which is what bevy's
stroke font carries.

## The zk contract

`zk` is the authority on the notebook, its templates and its filenames. Shell out; never
read `.zk/notebook.db` and never parse the markdown ourselves — a second implementation
is a second answer to what the notebook contains.

```
zk --no-input new --template=place.md --title=Riverford --extra=feature=7 --print-path
zk --no-input list --tag=place/riverford-a1b2 --format=json --quiet --sort=modified
zk tag list
zk edit <path>
```

**A tag no note carries prints nothing at all** — not `[]` — and exits successfully, so an
empty answer has to be decided before the output reaches a JSON parser. `--sort=modified`
is what orders the list: `modified` is RFC3339 with the fraction's trailing zeros trimmed,
so those strings do not compare bytewise and nothing here re-sorts them. The `Found N
notes` footer goes to stderr, so `--quiet` is tidiness rather than what makes the output
parse. A place note carries its own tag and so comes back in its own result set; it is
dropped by filename stem.

**A query runs only where there is already a `.zk`.** `zk` finds a notebook by walking up
from its working directory, so a campaign sitting inside the GM's own notes tree would
otherwise have that notebook answer. The reference query is also the one `zk` call with no
press behind it, so unlike `create` it never runs `zk init` — initialising a notebook as a
side effect of clicking a polygon is not something a GM asked for.

**The notebook watcher ignores `.zk/`.** `zk list` rewrites the index there whenever it
finds a note has changed, so a watcher that did not exclude it would be tripped by this
tool's own queries.

**Every invocation runs with the notes directory as its working directory**, because
`zk new` resolves the note it creates against that directory — naming the notebook is not
on its own enough, and `--notebook-dir` alone fails with *path is outside the notebook*.
`ZK_NOTEBOOK_DIR` is removed from the child: it redirects the note whenever the working
directory is not itself inside a notebook. **Every value is joined to its flag** with `=`;
a following word beginning with a dash is read as another option, so `--title "-Kai"` is
refused by `zk` itself. `--template` needs the file extension. Every call carries
`--no-input` with stdin closed — a child waiting on a prompt nothing can answer would hold
the one job slot for the life of the process.

Tags are `<kind>/<slug>`, for `place`, `person`, `faction` and `session`. **The slug is the
note's own filename stem**, which is what `{{filename-stem}}` renders — so two places
titled "Riverford" are `place/riverford-a1b2` and `place/riverford-c3d4`, and a GM who
changes `note.filename` in their own config does not break the tag. A place note's
frontmatter also carries its `FeatureId`, so the link survives a rename from either side.

Notes are opened in `zk edit` / `$EDITOR`, started and never waited on — an editor does not
return until the note is closed. **This tool is not a markdown editor.**

## Out of scope for v1

Named so they are not added by accident, not because they are bad ideas:

- Fog of war, per-feature `revealed` flags, and a player-facing window. The document
  format should stay open enough to add a `revealed` flag later without breaking saves.
- Editing note content in-app.
- Exporting a printable or player-facing image.
- Baking or authoring terrain. That is `watershed_editor`'s job and it already does it.
