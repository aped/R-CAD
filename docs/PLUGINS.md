# Plugins

A plugin adds feature types and analyses to R-CAD without changes to the document, command or
UI code. Plugins can be built into R-CAD, like **Plastics** (docs/PLASTICS.md), or installed as
external programs, like the example **Vent slots** (`examples/plugins/vent-slots`). The design
and its reasons are in ADR 0016.

## What a plugin is

- **A manifest** (`rcad_plugin::Manifest`): an id, a name, a version, the interface version
  (`api_version: 1`), and the feature types and analyses the plugin provides. Each one has:
  - inputs: `face`, `faces`, `body`, `sketch` or `points`, each named, labelled and optionally
    optional;
  - numeric parameters: `length` (mm), `angle` (degrees, the name ends in `_deg`), `count` or
    `ratio`, with a default, inclusive `min`/`max`, `exclusive_min`, and an optional `rule`
    template;
  - choices: named string options with a default.
- **A build**: `BuildRequest` → a list of operations. The request holds the complete parameters
  (defaults filled in, ranges checked), the choices, every input resolved to geometry at this
  point of the history, and the plugin's document data.
- Optionally:
  - **analyses** (`AnalysisRequest` → a JSON report);
  - **document data** with validation and effects: parameters to define and materials to
    assign;
  - **rule symbols**, which bind new features to rule parameters.

A build is deterministic and has no access to the document beyond its request.

### Resolved inputs

| kind | what the plugin receives |
|---|---|
| `face` | `frame` (origin on the face, `z` the outward normal), `body` id, `loops` (outer boundary first, then inner loops, exact lines/arcs in plane coordinates; empty if the boundary has other curves), `body_span` (the body's extent along `z` from its bounding box, conservative off-axis), `parallel` (every planar face of the body parallel to this one: `height` along `z`, `facing` ±1) |
| `faces` | a list of the above |
| `body` | `body` id and world bounding box |
| `sketch` | `frame`, solved non-construction `curves` (lines, arcs, circles), standalone `points` (used by no curve; not the sketch origin), the `body` whose face the sketch is on (or offset from), and that body's `parallel` planes |
| `points` | plane points as stored (in the plane of the feature's face input) |

`rcad_plugin::geom::{next_below, next_above}` find "to next" faces in `parallel`.

### Operations

One kind today:

```text
Prism { id, body, frame, profile, holes, from, to, taper_deg, mode, label }
```

- The region `profile` minus `holes` is drawn in `frame`'s plane coordinates.
- It is swept along `frame.z` over `[from, to]`.
- `mode` is `join` or `cut`, applied to the body of input `body`: a face or body input, or a
  sketch on a face.
- With `taper_deg` the walls are drafted: the section is exact at `from` and shrinks by
  `tan(taper)` per mm towards `to` (a negative taper grows). Drafted tools take one polygon or
  circle and no holes.
- Profiles: `circle`, `polygon` (any orientation) or `loop` (lines and arcs). The host puts
  them into canonical form (counter-clockwise, from the lowest vertex).
- `rcad_plugin::Profile::offset` is the sketch layer's exact offset: parallel lines, concentric
  arcs, mitred corners.

How the host applies operations:

- Each operation is checked first (`rcad_plugin::check_ops`). It is then applied through the
  same path as an Extrude Join/Cut and gated like one (ADR 0005). A failure names the
  operation's `label` and leaves the document unchanged.
- **Joins** starting at height 0 on a face of the target are joined like an extrusion sketched
  on that face.
- **Cuts** use their span as given. A plugin overshoots into air itself (e.g. `to = +1`), and
  only a far end landing exactly on a face is moved off it.
- To avoid coplanar boolean faces, plastics tools that stand on a face are sunk 0.2 mm into it.
  That material is joined anyway, and the result gate covers it.

**Operation ids** name the operation's faces for later features (`FaceRole::Plugin { op, of }`:
`EndCap`, `StartCap` or `Side { entity: i }`, where `i` is edge `i` of the canonical profile). Keep
ids stable across parameter edits by deriving them from what the operation is, never from
coordinates. A reference to an operation that is no longer built is a `BrokenReference`.

## Stored features

A plugin feature is `FeatureKind::Plugin`. It stores:

- `plugin`, `feature` and `plugin_version`;
- `inputs` (persistent references);
- `params` (each with its unit kind);
- `choices`.

Parameters are ordinary feature slots. `set_parameter`, `set_expression`, configurations and the
parameter table work on them, and `redefine_feature` edits every field in one undo step.

Format v10 stores plugin features, plugin face references and plugin data. It is written only
when a document uses them.

A document naming a plugin that is not installed still opens. That feature fails with "plugin
'id' is not installed" and its dependents are flagged. The definition is saved unchanged, and
the feature regenerates once the plugin is installed.

## Commands

| command | fields | result |
|---|---|---|
| `list_plugins` | | `Response.plugins`: each `{manifest, builtin}` |
| `plugin_feature` | `name?`, `plugin`, `feature`, `inputs` (name → `{"kind": "face", "plane": …}` / `{"kind": "faces", "faces": […]}` / `{"kind": "body", "body": 1}` / `{"kind": "sketch", "sketch": 4}` / `{"kind": "points", "points": [[x, y], …]}`), `params?` (name → number), `choices?`, `expressions?` (slot → expression), `use_rules?` (default true) | `Response.feature`. Parameters not given take their defaults. Each one with a rule template is bound to the rule of the feature's body, if that body has one (see below) |
| `set_plugin_data` | `plugin`, `data` (JSON, or `null` to remove) | Validated by the plugin. Its effects (parameters, materials) are applied in the same undo step |
| `run_plugin_analysis` | `plugin`, `analysis`, `inputs`, `params?`, `choices?`, `chord?` (mm, default 0.05), `use_rules?` | `Response.plugin_report` (JSON). Rule templates are evaluated to numbers for parameters not given |

Edit a plugin feature with `redefine_feature` (`kind`: the edited `FeatureKind::Plugin`).
`rcad-cli run SCRIPT.json` runs these like any command. `rcad-cli plugins [--json]` lists the
plugins.

### Rules and bindings

A plugin may expose rule symbols for a body (`Plugin::rule_symbols`), for example `T → ABS_T`.
When a feature is created with `use_rules`, every parameter that has a `rule` template and was
not given explicitly is bound to the expanded template. `T/2` becomes the expression `ABS_T/2`.
Changing the rule's data updates the parameters, and every bound feature follows. Parameters
given explicitly stay plain values.

## External plugins

An installed plugin is a directory with a `plugin.json`:

```json
{
  "id": "vent-slots", "name": "Vent slots", "version": "1.0.0", "api_version": 1,
  "command": ["python3", "-I", "plugin.py"], "timeout_secs": 10,
  "features": [ { "id": "vent_slots", "name": "Vent slots", "inputs": [ … ], "params": [ … ] } ]
}
```

**Search path.** R-CAD loads every subdirectory holding a `plugin.json` from the absolute
entries of `RCAD_PLUGIN_PATH` (`:`-separated; `;` on Windows), then from
`$XDG_DATA_HOME/r-cad/plugins` (default `~/.local/share/r-cad/plugins`; `%APPDATA%` on Windows).
The app and `rcad-cli run`/`plugins` load them at start. A built-in plugin id is never replaced.
When two directories use the same id, the first wins and the second is reported. Load errors
are reported and never stop the program.

**Manifest checks:**
- `plugin.json` must be a regular file of at most 1 MiB.
- `command` must have 1–256 entries, with no `..` components.
- A relative program with a `/` is resolved in the plugin directory and must exist. A bare name
  is looked up on `PATH`.
- `timeout_secs` must be 1–600 (default 30).

**Each call:**
1. The command starts in the plugin directory, with a cleared environment except `PATH`,
   `HOME`, `LANG`, `LC_ALL`, `LC_CTYPE` and `TMPDIR`, plus `RCAD_PLUGIN_API=1` and
   `RCAD_PLUGIN_DIR`. On Unix it runs in its own process group.
2. One JSON request plus a newline is written to its standard input, which is then closed.
3. It must write one JSON object to standard output and exit 0 within the timeout.
4. Output over 64 MiB, a timeout or a crash kills the process group and fails the call. The
   first 16 KiB of standard error is kept in the error message.

```text
-> {"method": "build",         "request": BuildRequest}       <- {"ok": [Op, ...]}
-> {"method": "analyze",       "request": AnalysisRequest}    <- {"ok": <report>}
-> {"method": "validate_data", "data": <json>}                <- {"ok": null}
-> {"method": "data_effects",  "data": <json>}                <- {"ok": {"parameters": [...], "materials": [[body, key]]}}
-> {"method": "rule_symbols",  "data": <json>, "body": 3}     <- {"ok": {"T": "ABS_T"}}
any method                                                    <- {"error": "why"}  (refused, shown to the user)
```

Wire shapes follow the serde tags:
- operations: `{"op": "prism", …, "mode": "join"|"cut"}`;
- profiles: `{"shape": "circle"|"polygon"|"loop", …}`, with loop segments
  `{"seg": "line"|"arc", …}`;
- resolved inputs: `{"kind": "face"|"faces"|"body"|"sketch"|"points", …}`.

Answers are untrusted. They are parsed into typed values and checked before use, and an answer
that is neither `ok` nor `error` fails the call.

**Trust.** R-CAD contains a misbehaving plugin program but does not sandbox it. An installed
plugin runs with your user's permissions, as Fusion add-ins do. Install plugins you trust.
Documents never contain plugin code.

**Writing one in Python.** `examples/plugins/standoffs/rcad_plugin.py` is a one-file,
standard-library helper. Copy it next to your script. It reads the request, dispatches to
`@plugin.feature("id")` / `@plugin.analysis("id")` functions, answers `Refusal("why")` as a
refusal, and provides profile, frame and `prism` helpers.

**Examples.**
- `examples/plugins/standoffs` uses the helper: posts with pilot holes at `points` on a face.
- `examples/plugins/vent-slots` speaks the protocol directly: rounded slots cut into a face. `crates/rcad-commands/tests/it/external_plugin.rs` uses both through a document: creation
against closed-form volumes, a parameter edit, a plugin refusal, then save and reload.

## Writing a built-in plugin

1. Add a crate depending on `rcad-plugin` and implement `rcad_plugin::Plugin` (`manifest`,
   `build`, and optionally `analyze`, `validate_data`, `data_effects`, `rule_symbols`).
2. Register it in `rcad_commands::plugins::register_builtin` and list its id in `BUILTIN`.
3. Test through the command path with closed-form volumes and probes (see
   `crates/rcad-commands/tests/it/plastics/`). Unit-test the manifest (`Manifest::validate`) and
   the rule templates.

## Relation to Fusion add-ins

| Fusion | R-CAD |
|---|---|
| add-in folder with a `.manifest` | plugin directory with `plugin.json` (external) or a compiled-in crate |
| command definition with dialog inputs | feature schema; the app's generic plugin dialog renders it |
| custom feature (one timeline node, own parameters, built from native features) | `FeatureKind::Plugin`: one history item, parameters as slots, built from checked operations |
| model parameters created by plastic rules | user parameters from `data_effects` |

Fusion's Python/C++ `adsk` API scripts do not run in R-CAD. They would need a compatibility layer
on top of this interface.
