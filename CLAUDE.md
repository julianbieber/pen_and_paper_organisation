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
notebook changes underneath us. The milestone is filed as issues #1–#11; the design
behind them is in the plan at
`~/fun_repos/hobby-mimisbrunnr/notes/pen_and_paper_organisation/init/pen_and_paper_plan/plan-2026-08-30-campaign-map-and-notes.md`.

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
repo writes a terrain, bakes one, or reaches for `watershed_editor`. Two files, two
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

## Cities are on the world map

A settlement is a polygon on the world map, and the tavern inside it is a point whose
`parent` is that polygon. It is drawn once the parent covers enough screen area, so world
zoom stays readable without a child document or a view switch. **Dungeons** are child
documents, because a 30-metre room has no meaningful position at terrain-cell scale.

## Layout

```
crates/campaign          model, serde, io, zk client — NO bevy, tested without a GPU
crates/campaign_editor   the bevy app, binary `pnp`

my-campaign/
├── campaign.ron         version, name, terrain dir, units_per_cell, unit
├── terrain/             watershed output, read-only
├── world.ron            features
├── dungeons/*.ron       child documents
├── images/              imported backdrops, copied in so the directory stays portable
└── notes/               zk notebook; .zk/templates/ holds place, person, faction, session
```

The crate split is the one `watershed` uses and exists for the same reason: the model is
testable without a window. **Anything that can live in `campaign` does.** A rule that
needs a GPU to test is a rule that will not be tested.

`terrain/` and `world.ron` sitting inside the campaign directory is the **convention the
dialog pre-fills, not a guarantee the layout enforces**: `Campaign::create` writes neither.
It records where a terrain is — which may be an absolute path anywhere on the machine —
and the world document belongs to #3. **One terrain per campaign**: `campaign.ron` has a
single `terrain` field, and `open` loads it eagerly, so a second would be a second load.

**A `Campaign` is immutable after `open`.** It holds what is on disk — root, manifest,
terrain. The undo stack, the dirty flag, the notebook handle and any tile cache are
editor-session state and never fields of it, so #3's mutable world document is its own
type rather than a `ResMut<Campaign>` serialising against #2's terrain reads every frame.

**An absent `world.ron` is an empty world, not an error** — `create` writes none, so #3
reads "missing" as "default" rather than inventing a migration.

**Nothing in this repo ever calls `Terrain::save_to_dir`.** It deletes `layer_<n>.png`
files it does not name, and terrain here is read-only.

`create` still makes a plain `notes/` directory and runs no subprocess. `campaign::notebook`
makes it a notebook the first time a note is asked for (#6): `zk init` only when there is no
`.zk`, then the four templates ensured on every create, each written only where no file of
that name is there — so a notebook the GM made themselves gains them, one whose templates
were half-written repairs itself, and a template the GM has edited is never overwritten.

**`crates/campaign` still passes its tests with nothing on `PATH`.** The argument vectors
are pure functions tested against a recorded `Runner`; the handful of tests wanting the real
program live in `tests/notebook_zk.rs` and skip themselves, with `PNP_REQUIRE_ZK=1` turning
the skip into a failure so CI cannot pass having checked none of it.

## Conventions

- **Coordinates are terrain cells** everywhere in a world document, so a feature and a
  terrain sample agree without a conversion at every call site. `campaign.ron` holds
  `units_per_cell` for anything the GM reads.
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

**The tileset is drawn by hand in `bevy_sprite_editor`** (`~/fun_repos/bevy_sprite_editor`)
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
