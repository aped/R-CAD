# ADR 0016: Plugins: declared features built from checked operations

Status: accepted (2026-10-23)

## Context

Every modelling feature so far is a variant of the closed `FeatureKind` enum. A new feature
touches about fifteen places: the definition, regeneration, parameters and slot kinds, the
native format, the command enum, the UI registry, dialogs and Edit Feature. That is right for
core features. It does not scale to families of domain features such as plastic part design,
fasteners or vents, and it gives users no way to add features of their own.

Autodesk Fusion is the behavioural reference. It has add-ins: a folder with code and a JSON
manifest, commands with dialogs built from declared inputs, and (as an API preview) custom
features that are one timeline node with their own parameters, built from native features.
Fusion's plastic features (Boss, Snap Fit, Lip, Rest and the plastic rules) are part of an
extension, not core commands.

The invariants that must hold for plugins as for core features:
- history is authoritative (definitions and parameters are stored, not a result mesh);
- regeneration is deterministic;
- persistent references are semantic and never face indices;
- results are exact and checked (ADR 0005);
- a failure leaves the previous valid state;
- every action is reachable through the shared command API.

## Decision

1. **A plugin declares, it does not draw.** `rcad-plugin` defines a `Manifest`. It lists
   feature types and analyses with typed inputs (face, faces, body, sketch, points), numeric
   parameters (length, angle, count, ratio, with bounds and an optional rule template) and named
   choices. The document, the command API, the CLI and the UI's generic plugin dialog read only
   the manifest.

2. **One stored variant for every plugin feature.** `FeatureKind::Plugin` holds:
   - the plugin and feature ids, and the plugin version;
   - the inputs as persistent references (`PluginInput`: a sketch plane on a face, face
     references, body ids, sketch feature ids, plane points);
   - the parameters with their unit kind, so the slots stay editable and bindable without the
     plugin;
   - the choices.

   Parameters are ordinary feature slots: `set_parameter`, expressions, configurations and Edit
   Feature (`redefine_feature`) work on them unchanged.

3. **A build is a pure function.** On every regeneration the document resolves the inputs to
   geometry (`Resolved`):
   - for faces: the frame, the exact boundary loops, the body's span along the normal and the
     body's parallel planar faces;
   - for sketches: the solved curves, the standalone points, the face body and its parallel
     planes.

   The document then calls `Plugin::build(&BuildRequest) -> Vec<Op>`. The request carries the
   complete parameters, the resolved inputs and the plugin's document data, and nothing else.
   There are no callbacks into the document. A build therefore runs the same in process and out
   of process, and regeneration stays deterministic.

4. **One operation vocabulary, executed by the host.** `Op::Prism` is a plane region with
   holes, swept along a frame normal over an explicit span, optionally drafted, and joined to or
   cut from the body of an input. The host checks every answer (`check_ops`: unique ids, input
   bodies, orthonormal frames, finite increasing spans, valid profiles). It then applies each
   operation through the same path as an Extrude Join/Cut: exact slab construction or Truck
   boolean, then the ADR 0005 result gate. Drafted tools use the tapered-boolean path. A plugin
   cannot create unchecked geometry. A failing operation fails the feature with the operation's
   label, and the transaction keeps the previous state.

   Two more kinds map to existing checked kernel paths:
   - `Op::EdgeLoop`: the exact cap-loop fillet/chamfer of an earlier operation's cap;
   - `Op::ReplaceFace`: the exact topology-preserving face replacement of ADR 0010, used for
     drafts.

   Further kinds (revolve, new bodies) are additions to this enum on the same terms.

5. **Faces of plugin features are referenceable.** `FaceRole::Plugin { op, of }` names the end
   cap, start cap or side `i` of operation `op`, resolved like a pattern occurrence's face: from
   the stored tool, validated on the B-rep, and ambiguous or missing faces are a
   `BrokenReference`.

   Operation ids are the plugin's to keep stable. They derive from what an operation is (point
   index × 32 + part), not from coordinates, so a parameter edit keeps references.

6. **Plugin document data with effects.** `Document.plugin_data[plugin]` is opaque JSON that
   the plugin validates (`validate_data`) before it is stored. Its declared effects
   (`data_effects`) are applied in the same undo step: user parameters to create or update, and
   catalog materials to assign to bodies.

   Rules work as in Fusion: a rule becomes parameters (`ABS_T`, `ABS_DA`, …). A new feature on a
   body with a rule binds each parameter that has a rule template (`T/2`) and was not given
   explicitly to the expanded expression (`ABS_T/2`). Editing the rule then regenerates every
   bound feature through the ordinary expression machinery.

7. **Built-in and external plugins share one registry.** Built-in plugins (`plastics`) are
   compiled in and registered by `Session::new`. External plugins are directories with a
   `plugin.json` (manifest plus `command`) in the plugin search path. They are served by a
   program that answers one JSON request per call (`rcad_plugin::process`). Calls are bounded:
   timeout, output size, a cleared environment and their own process group. Answers are
   untrusted and pass the same checks.

   Documents never carry plugin code. A document naming a plugin that is not installed opens
   with that feature failed by name ("plugin 'x' is not installed"). Its definition is kept and
   saved unchanged, and it regenerates once the plugin is installed.

8. **Analyses are read-only plugin calls.** The host hands an analysis the exact face kinds and
   a closed tessellation of each body input, labelled as an approximation within its chord. The
   report is plugin-defined JSON.

9. **Format v10** holds plugin features, plugin face references and plugin data. It is written
   only when a document uses them.

## Consequences

- New domain features need no change to the document, command or UI code, only a manifest and
  a build. The plastics plugin is about 2,100 lines for its rules and six features, plus about
  1,000 for its analysis.
- What a plugin can build is bounded by the operation vocabulary. Plastic features that need
  fillets at their roots, swept lips along arbitrary edge chains, or curved ribs are refused or
  documented as deviations until those operations exist. A plugin cannot approximate them.
- Operation execution reuses the exact paths, so their limits apply to plugins too. Joins
  starting on a face must lie within it, or the target must be prismatic along the tool. Tapered
  tools take one polygon or circle.
- External plugins run user-installed programs, as Fusion add-ins do. The host contains their
  failures, but it does not sandbox what they can read or write. That is the installer's trust
  decision, stated in docs/PLUGINS.md.
- Fusion add-in scripts (Python against the `adsk` API) do not run here. Their concepts
  (manifest, declared command inputs, custom features as one timeline node with parameters) map
  onto this model. An `adsk` compatibility layer would sit on top of this interface, not replace
  it.
