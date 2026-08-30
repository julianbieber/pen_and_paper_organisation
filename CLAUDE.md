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
campaign. Nothing draws a map yet. The milestone is filed as issues #1–#11; the design
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

`create` makes a plain `notes/` directory. Making it a `zk` notebook and installing the
templates is #6's — `crates/campaign` must not need `zk` on `PATH` to pass its tests.

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

Worked example: `~/fun_repos/watershed/crates/watershed/examples/load_terrain.rs`.

## The zk contract

`zk` is the authority on the notebook, its templates and its filenames. Shell out; never
read `.zk/notebook.db` and never parse the markdown ourselves — a second implementation
is a second answer to what the notebook contains.

```
zk new notes --template place --title "Riverford" --print-path
zk list --tag place/riverford --format json
zk tag list
zk edit <path>
```

Tags are `place/<slug>`, `person/<slug>`, `faction/<slug>`. A place note's frontmatter
also carries its `FeatureId`, so the link survives a rename from either side.

Notes are opened in `zk edit` / `$EDITOR`. **This tool is not a markdown editor.**

## Out of scope for v1

Named so they are not added by accident, not because they are bad ideas:

- Fog of war, per-feature `revealed` flags, and a player-facing window. The document
  format should stay open enough to add a `revealed` flag later without breaking saves.
- Editing note content in-app.
- Exporting a printable or player-facing image.
- Baking or authoring terrain. That is `watershed_editor`'s job and it already does it.
