# Changelog

All notable changes to R-CAD. Versions follow [Semantic Versioning](https://semver.org/);
the native document format has its own version number (`"version"` in `*.rcad.json`), and
which format versions a build reads and writes is listed in
[`docs/FORMAT_SUPPORT.md`](docs/FORMAT_SUPPORT.md).

## [Unreleased]

### Added

- **Plugins.** Feature types and analyses can come from plugins: built in, or installed as
  external programs that answer JSON requests (`plugin.json` in the plugin search path, e.g. the
  Python example `examples/plugins/vent-slots`). A plugin declares inputs, parameters and
  choices; R-CAD resolves the inputs on every regeneration and applies the plugin's operations
  through the same checked path as Extrude Join/Cut, so plugin geometry is exact and
  result-checked. Plugin features are one history item with ordinary parameter slots, faces
  later features can reference, and Edit Feature; a document naming a plugin that is not
  installed opens with that feature failed by name and keeps it. Commands `list_plugins`,
  `plugin_feature`, `set_plugin_data`, `run_plugin_analysis`; `rcad-cli plugins`. Native format
  v10 (written only when used). See `docs/PLUGINS.md`, ADR 0016.
- **Plastics** (built-in plugin):
  - plastic rules: named nominal values that become parameters and bind new features, with
    material presets; assignable to bodies or assembly components; they also drive core Shell,
    Fillet, Extrude taper/thin and Thicken (`bind_rule_parameters`);
  - Boss: pilot/through holes, draft, gusset ribs, tip fillet, and a mating boss with clearance
    hole and counterbore;
  - Rib and Web: along lines and arcs, draft, to next;
  - Snap Fit: hook and groove or hook and loop, perpendicular or stand-off;
  - Lip: lip, groove, or lip and groove;
  - Rest: from loops of lines and arcs;
  - Draft of existing faces (fixed plane);
  - analyses: Design Advice (thickness, variation, draft, undercuts, knife edges), Draft
    Analysis, Accessibility Analysis and Minimum Radius Analysis.

  See `docs/PLASTICS.md`.
- Material catalog: PC, PC/ABS, PP, PA66, POM, HDPE.
- **Add-in API scripts.** `rcad-cli script SCRIPT.py [--out DOC] [--commands OUT.json]`
  runs Python scripts written for a subset of the add-in scripting API surface: sketches on
  construction planes and faces, extrude, shell, fillet, user parameters, and the boss, rib,
  web and plastic rule objects. A pure-Python `adsk` package (`compat/scripting/adsk`, an
  independent implementation from the public API reference) records the calls as R-CAD
  commands, converting from centimetres and radians and binding parameter expressions. The
  CLI then executes them like `run`. Members outside the subset fail by name. See
  `docs/SCRIPTING.md` and the examples in `compat/scripting/examples`.
- **Add-ins, headless.** `rcad-cli addin ADDIN_DIR [--list] [--command ID]
  [--input ID=VALUE ...] [--base SCRIPT.py]` checks the add-in's manifest, calls its `run`, and
  runs one of its commands without a user interface: command definitions, events and handlers,
  value, boolean, integer, drop-down, string, text and selection inputs, validation, a discarded
  preview, then `execute` and `stop`. `--list` prints the commands and their inputs as JSON.
  Example: `compat/scripting/examples/addins/PlasticBoxAddin`.

- **Optional sync server client.** R-CAD stays complete offline with no account; adding an
  account for a self-hosted server (protocol in `crates/rcad-cloud-proto`, documented routes,
  JSON over HTTPS) adds a Data panel with organizations, projects and folders, Save Version
  (numbered server versions, milestones, conflict detection against the version you started
  from), Version History (open read-only, promote), edit locks taken on the first edit,
  share links, comments, and an offline queue that uploads saved versions later.
  `rcad-cli cloud` does the same from scripts. Builds without the `cloud` feature contain
  none of it. See `docs/CLOUD.md`.

## [1.0.0] - 2026-10-15

First stable release. From 1.0 on, the command API (`docs/COMMANDS.md`), the CLI and the
native format follow semantic versioning: a document saved by 1.x opens in every later 1.x
build, and breaking changes to commands or files wait for 2.0.

### What 1.0 contains

- **Sketching:** lines, rectangles, circles, arcs, ellipses, splines, slots, polygons, text;
  fillets, offsets, trim/extend, sketch patterns; projection and intersection of model
  edges; dimensions (driving and driven) and geometric constraints with a solver that
  reports residuals, remaining degrees of freedom and conflicts; regions from any
  arrangement of crossing curves.
- **Solids:** extrude, revolve, loft, sweep, holes, threads (faceted), rib, shell,
  fillets and chamfers (straight edges and whole cap loops), linear/circular patterns,
  mirror, body move/copy/scale/split/combine, sheet-metal contour flange with flat pattern,
  direct edits of imported STEP faces. Cuts that separate a body produce one body per piece,
  with stable piece identity across edits.
- **History:** parametric feature history with named parameters and expressions,
  suppression, rollback, reorder, Edit Feature, configurations, per-feature health with
  partial regeneration and safe-mode open, transactional commits, undo/redo.
- **Assemblies, drawings, inspection, mesh tools, print export:** see the README feature
  table.
- **Interchange:** STEP (exact B-rep subset, assemblies, surface bodies), STL, 3MF, DXF,
  SVG; native format v9 with migration from every earlier version.

### New in the 1.0 release cycle

- **Faster booleans and regeneration.** Intersection curves reuse a cached per-surface
  presearch grid (bit-identical results), tessellation maps use a faster hasher, the
  post-boolean result check reuses meshes and indexes profile polygons. Kernel-heavy
  regenerations are 2-7x faster; a plate cut regenerates in about 0.1 s. Bounds and
  tessellations of unchanged bodies are reused, so a parameter edit on the rounded
  enclosure template takes about a third less time; hover and click picking use a
  bounding-volume hierarchy (milliseconds to microseconds on large meshes).
- **Release binaries** are built with a `dist` profile (fat LTO, one codegen unit): 5-10 %
  faster and about a quarter smaller.
- **Edit Feature** reopens a prefilled dialog for fillets, chamfers, shells, patterns,
  mirror, loft, sweep, thread and align, not only extrude, revolve and hole.
- **Messages:** an action that cannot run says why over the 3D view; error banners say
  what failed and what to do next.
- **Discoverability:** menus show keyboard shortcuts, disabled buttons name their key,
  the empty-document card offers templates, and **File > About R-CAD** shows the version.
- **Status bar and title** show one save state (not saved yet / unsaved changes / saved).
- **File > Quit** (Ctrl+Q) with the unsaved-changes prompt; Esc closes About/Help/
  Preferences first; every dialog value shows its unit; faces and sketch planes have plain
  names ("Extrude1 end cap"); dragged panel widths are remembered.
- Icons the UI font could not draw (shown as empty boxes) are replaced.
- **Light theme** is complete: the 3D view, grid, sketch geometry, overlay labels and status
  colours follow the Light/Dark/Follow system preference (status text meets 4.5:1 contrast
  on the light panel). `RCAD_THEME=light|dark|system` overrides it for one run.
- **Alignment:** every menu uses one right-aligned shortcut column and a common minimum
  width; ribbon group titles sit horizontally under their buttons and captions are no
  longer cut off; docked dialogs use a two-column form (labels left, OK/Cancel right);
  History names line up in one column.
- **Packages for each platform:** an application icon; a universal macOS `R-CAD.app`
  (Apple silicon and Intel) in a `.dmg`; a Windows `rcad.exe` that opens no console window
  and carries its icon and version information; Linux archives for x86_64 and ARM64 with a
  desktop entry and `install.sh`, and an AppImage for each. `rcad <document>` opens a document at startup. Every release package has a Sigstore-signed
  build provenance attestation (`gh attestation verify <file> -R aped/R-CAD`).
- **CI:** a fast lint job, one test binary per crate (test build about 4x faster, test
  archive 10x smaller), sharded nextest runs, Linux, Windows and macOS all test-gated,
  and release archives with SHA-256 checksums. A Linux leg takes about 11 minutes, down
  from 35.

### Breaking changes since the 0.x prototype

- Documents must declare `"format": "r-cad"`. Files written under the project's former
  name are no longer recognised; re-save them with a 0.x build, or change the `format`
  field by hand (the rest of the file is unchanged).
- The former-name autosave locations and environment variables are no longer read; use
  `RCAD_RECOVERY_DIR` and `RCAD_AUTOSAVE_SECS`.
- Workspace crates are versioned together at 1.0.0 and are not published to crates.io.
