# Plastics

The built-in **Plastics** plugin (`crates/rcad-plugin-plastics`, plugin id `plastics`) covers
plastic part design:
- plastic rules (on bodies or assembly components), which also drive core Shell, Fillet,
  Extrude taper/thin and Thicken;
- Boss, Rib, Web, Snap Fit, Lip, Rest and Draft;
- Design Advice, Draft Analysis, Accessibility Analysis and Minimum Radius Analysis.

The behavioural reference is the Plastic tab of Autodesk Fusion's Design workspace (Fusion
documentation; no Fusion code or assets are used). Each feature is built from checked operations
(docs/PLUGINS.md: prisms, edge-loop blends, face replacement): results are exact B-rep and pass the same result gate as Extrude
Join/Cut. What that cannot build exactly is refused or listed below as a deviation, never
approximated.

In the app the features are on the **PLASTIC** ribbon tab (docs/UI.md). From scripts and the CLI
they are `plugin_feature` commands with `"plugin": "plastics"`. Each section below lists the
feature's inputs, parameters and choices. Defaults in parentheses; *rule* marks the template a
rule binds the parameter to.

## Plastic rules

A rule is a named set of nominal design values. Each value is an expression over the rule's own
symbols:

| symbol | meaning | default |
|---|---|---|
| `T` | nominal wall thickness | `2 mm` |
| `DA` | nominal draft angle, degrees | `1` |
| `NR` | nominal inner radius | `0.5*T` |
| `CL` | clearance between mating features | `0.1 mm` |
| `RH` | lip height (reveal) | `T` |
| `TMIN`, `TMAX` | design advice thickness limits | `0.5*T`, `1.5*T` |
| `TV` | allowed thickness variation (ratio) | `0.15` |
| `MDA` | design advice minimum draft, degrees | `0.5` |
| `KE` | knife-edge threshold | `0.5*T` |

**Presets.** These are starting points with typical guidance, not supplier data. Each sets `T`
and `DA` and a catalog material (docs/MATERIALS.md):

| preset | T | DA |
|---|---|---|
| ABS, ASA, PC, PC_ABS | 2 mm | 1° |
| PP | 1.5 mm | 1° |
| PA66, POM | 1.5 mm | 0.5° |
| HDPE | 2 mm | 1.5° |

**Data.** The rules live in the document as the plugin's data:

```json
{"rules": [{"name": "ABS", "material": "abs", "values": {"T": "2 mm", "DA": "1"}}],
 "bodies": {"1": "ABS"}}
```

Store it with `set_plugin_data`. It is validated before it is stored: rule names are
identifiers and unique, values use known symbols only, assigned rules exist, and materials are
catalog keys.

**Effects.** In the same undo step:
- every rule value becomes a user parameter, `<rule>_<symbol>` (`ABS_T = 2 mm`,
  `ABS_NR = 0.5*ABS_T`);
- every assigned body gets the rule's material.

**Binding.** A plastics feature created on a body with a rule binds each parameter it was not
given to the parameter's rule template. A web's thickness becomes `ABS_T`; a boss's rib
thickness becomes `0.6*ABS_T`. Editing the rule updates every bound feature, and undo restores
it.

As in Fusion, features created before the rule was assigned keep their plain values.

**Components.** As in Fusion, a rule can be assigned to an assembly component
(`"components": {"<id>": "ABS"}`). Its bodies take the rule's material and new features on them
bind to it; a body's own assignment wins over its component's.

**Core features.** `bind_rule_parameters {feature}` binds a core feature's rule-driven slots to
its body's rule: Shell thickness → `T`, Fillet radius → `NR`, Extrude taper → `DA` and thin wall →
`T`, Thicken distance → `T`. Slots already bound to an expression are kept. The app's dialogs do
this when "Use plastic rule" is on.

## Boss

Screw bosses at the standalone points of a sketch (or, without points, at its circle centres).
The sketch must lie on a face of the boss body, or on a plane offset from one.

**Inputs:** `sketch`; `mate` (optional): the inner face of the other half, parallel to the
sketch and facing it.

**Parameters:**
- shank: `height` (8), `diameter` (6), `draft_deg` (1, *rule* `DA`);
- hole: `hole_diameter` (2.5, at the top), `hole_depth` (6, from the top), `hole_draft_deg`
  (0);
- ribs: `rib_count` (0–12), `rib_thickness` (1.2, *rule* `0.6*T`), `rib_length` (6, from the
  axis), `rib_height` (6), `rib_chamfer` (3), `rib_tip_height` (2), `rib_angle_deg` (0), `rib_total_deg` (360);
- mating boss: `mate_diameter` (6), `mate_hole_diameter` (3.4), `cbore_diameter` (6),
  `cbore_depth` (0: none), `csink_angle_deg` (90), `gap` (*rule* `CL`);
- `tip_radius` (0: none): fillet of the shank top's outer edge;
- alignment step: `step_diameter` (4.5), `step_depth` (1).

**Choices:** `hole`: `pilot` | `through` | `none`; `step` (side 1): `none` | `out` | `in`; `seat`: `counterbore` |
`countersink`.

**Result.** Per point:
- a drafted shank from the base face (the nearest face at or below the sketch plane facing it)
  up `height`;
- gusset ribs, a section from the axis to `rib_length` with a flat top at `rib_height`,
  chamfered over the last `rib_chamfer` down to `rib_tip_height` (Fusion's chamfer profile),
  spread evenly over the total angle. The chamfer must start outside the shank: the flat top
  then meets the shank in circle arcs and the gusset sides in straight lines, all exact edges, so
  the body exports to STEP;
- the hole, cut last.

With `mate`, the other body also gets:
- a mating shank from the mate face to `gap` short of the shank top;
- a clearance hole through the mate body;
- a counterbore from its outside face, or with `seat` = `countersink` an exact cone of
  `cbore_diameter` at `csink_angle_deg` (built into the topology by the kernel).

**Refused:**
- a hole not narrower than the drafted top;
- a pilot deeper than the material;
- ribs on a drafted shank (the kernel cannot join planar ribs to a cone yet; use a straight
  shank with ribs);
- ribs on a base thinner than 0.4 mm;
- a mate face that is not parallel and facing;
- no room for the mating shank.

**Alignment step** (Fusion's Step In/Out). `out` puts a ring of `step_diameter` × `step_depth`
on each shank top and a recess `gap` wider and deeper in the mating boss; `in` is the reverse.

**Tip fillet.** An exact cap-loop blend of the shank top, applied last (docs/FILLETS.md). The
kernel refuses it where gussets reach the top, the hole is within the radius of the edge, or the
shank is drafted.

**Not modelled:** root fillets (the kernel blends convex cap loops, not the concave base edge),
drafted ribs, fastener components.

## Rib and Web

Thin walls from open chains of sketch lines and arcs.

**Inputs:** `sketch`; `body` (when the sketch is not on a face).

**Parameters:** `thickness` (*rule* `T`), `depth` (10), `draft_deg` (*rule* `DA`).

**Choices:** `thickness_side`: `symmetric` | `one_side`.
- Rib also has `side`: `left` | `right`, and `extent`: `distance` | `to_next`.
- Web also has `direction`: `normal` | `flip`, and `extent`: `distance` | `to_next`.

**Rib.** The thickness runs across the sketch plane. The material grows in the plane from each
line by `depth`, towards `left` (the sketch normal × the line direction) or `right`. With
`to_next` it grows to the nearest planar face of the body in that direction, found by rays from
points along the curve; the face must be at one distance along the whole curve (a constant-depth
rib cannot meet a slanted face exactly), else the rib is refused with the distances. Draft
narrows the rib towards its far edge.

**Web.** The thickness runs across the line in the sketch plane. The material grows along the
sketch normal by `depth`, or with `to_next` to the nearest face of the sketch's body facing it
(sunk 0.2 mm into that face). Draft narrows the web away from the sketch plane.

**Corners.** Lines meeting at a point are each extended by half the thickness there, so corners
are filled.

**Arcs.** A rib or web along an arc is the exact annular sector about the arc's centre (a web
`thickness` wide across it; a rib grown towards or away from the centre).

**Refused:** circles; drafted ribs and webs along arcs (the drafted wall would be a cone
section); a draft that closes the wall before its depth.

**Not modelled:** root fillets (concave blends), Extend Curves.

## Snap Fit

Cantilever hooks at sketch points, with an optional mating groove or loop.

**Inputs:** `sketch`; `mate` (optional): the body that receives the groove or loop.

**Parameters:**
- beam: `length` (8), `thickness` (*rule* `T/2`), `tip_thickness` (*rule* `T/2`), `width` (5),
  `rotation_deg` (0);
- catch: `catch_depth` (*rule* `T/3`), `catch_height` (*rule* `T`), `insertion_deg` (30),
  `retention_deg` (90);
- stand-off: `base_depth` (5);
- mating feature: `clearance` (*rule* `CL`), `loop_thickness` (1.5), `loop_length` (3).

**Choices:** `type`: `hook_and_groove` | `hook_and_loop`; `orientation`: `perpendicular` |
`parallel`; `extent`: `to_next` | `distance`; `orient`: `uniform` | `outward` | `inward`.

**Hook.** The hook is a section standing on the sketch plane, swept across its `width`:
- the beam tapers from `thickness` at the root to `tip_thickness` at `length`;
- the catch at the tip protrudes `catch_depth`, with a retention face at `retention_deg` and an
  insertion face at `insertion_deg`, both measured to the beam axis. 90° retention is a flat,
  permanent ledge.

**Rotation.** `uniform` turns every catch to `rotation_deg`. `outward` and `inward` point each
catch away from or towards the centroid of the points, plus `rotation_deg`. This is a fixed-point
form of Fusion's Aligned rotation.

**Root.**
- `perpendicular` hooks stand on the sketch's face.
- `parallel` hooks stand off a lower face: the beam continues from the sketch plane to the
  nearest face facing it (`to_next`) or by `base_depth`.

**Mating body.**
- A groove cuts the catch's envelope, grown by `clearance`, from the mate.
- A loop joins a bar under the retention face. It runs from `clearance` beside the beam to
  `loop_length` past the catch, and must reach the mate body.

**Refused:** a catch too short for its angles; a catch longer than the beam.

**Not modelled:** base fillet, deflection/strain checks (Fusion has none either), Fusion's
lettered advanced dimensions.

## Lip

A lip and/or groove along the rim of a shelled part. The rim is a planar face with one outer and
one inner boundary loop.

**Inputs:** `face` (the rim); `mate` (the other half's rim, for `lip_and_groove`); `extents`
(optional: two points on the rim).

**Parameters:** `width` (*rule* `T/2`), `height` (*rule* `RH`), `clearance` (*rule* `CL`).

**Choices:** `type`: `lip` | `groove` | `lip_and_groove`; `position`: `inner` | `outer`.

**Result.**
- **Lip:** an exact offset band of the rim boundary, `width` wide along the inner or outer
  loop, raised `height`.
- **Groove:** the same band grown by `clearance` on each side, cut `height + clearance` deep.
- **Lip and groove:** the lip on `face`, the groove on `mate`, where `mate` faces `face`.

**Refused:** a band not narrower than the wall; a face that is not a rim; a groove deeper than
the part.

**Extents.** With two points, the lip (and its groove) runs only along the rim from the first
point to the second, counter-clockwise seen from outside the face. The band's ends are square,
with exact offset lines and arcs and mitred corners. The rim face and `position` stand for
Fusion's edge chain and face guide.

**Not modelled:** Trapezoidal (drafted)
sections are not available yet: the exact constructions tried (a tapered tool across the cavity,
per-edge trapezoids, per-edge draft wedges) fail in the boolean kernel at the rim corners.
The rim face stands for the edge chain.

## Rest

A flat platform inside a shelled part, with uniform walls, so it makes no thick section.

**Inputs:** `sketch`: closed profiles (circles, loops of lines and arcs) on a plane offset from a
face of the body, above a floor. Drafted rests take circles and line loops.

**Parameters:** `thickness` (*rule* `T`), `offset` (0, the top relative to the sketch plane),
`depth` (10), `draft_deg` (*rule* `DA`).

**Choices:** `extent`: `to_next` | `distance`.

**Result.** For each profile:
- the profile is joined from the floor up to the top: the nearest face below facing it
  (`to_next`), or `depth` below the sketch plane;
- with draft, the profile is exact at the top and grows towards the floor;
- its inside, `thickness` in from the walls, is cut from below the floor up to `thickness`
  under the top.

So the platform is a cap on walls whose inside opens through the floor: a uniform wall and no
enclosed void.

**Refused:** a thickness that does not fit the profile; a top not above the floor by more than
the thickness; profiles with arcs.

**Not modelled:** Fusion's top-face limits, trimming at the body, per-side drafts. Wall
thickness is measured across the walls in the sketch plane.

## Draft

Fusion's fixed-plane draft of existing faces.

**Inputs:** `faces` (planar faces to draft), `fixed` (the fixed plane, a face of the body).

**Parameters:** `draft_deg` (1, *rule* `DA`, 0–30).

**Choices:** `direction`: `pull` | `reverse`.

**Result.** Each face turns about its line on the fixed plane by the angle, relative to its
current orientation, leaning in as it goes along the pull direction (away from the fixed face,
into the part); `reverse` leans it out. Built with the kernel's exact face replacement, which
keeps the body's topology and checks the result.

**Refused:** a face parallel to the fixed plane, a face on another body, and faces the kernel's
face replacement does not take (not a convex single loop of straight edges with planar
neighbours, or a vertex with more than one side edge, e.g. a wall with a hole or a filleted
corner).

**Not modelled:** parting-line drafts, two-sided and symmetric drafts, tangent chains.

## Design Advice

A read-only analysis (`run_plugin_analysis`, `"analysis": "design_advice"`) of one body for a
pull direction.

**Inputs:** `body`; `pull` (optional face: its normal is the pull direction).

**Parameters:** `min_thickness` (*rule* `TMIN`), `max_thickness` (*rule* `TMAX`), `variation`
(*rule* `TV`), `min_draft_deg` (*rule* `MDA`), `knife_edge` (*rule* `KE`).

**Choices:** `pull_axis` (`+z`, …, used without a pull face).

**Report (JSON):**
- **thickness:** a ray from each triangle along the inward normal to where it leaves the
  material; min/max/mean, areas too thin and too thick, per-face values, worst locations;
- **variation:** neighbouring triangles on a smooth surface differing by more than `variation`;
- **draft:** per face, `asin(|n·pull|)`:
  - exact for planes, and for cylinders and cones coaxial with the pull;
  - other faces take the minimum over their triangles;
  - faces below `min_draft_deg` are flagged;
- **undercut:** triangles that can be released neither along `+pull` nor along `−pull`
  (two-plate mould, no side actions);
- **knife_edge:** regions thinner than `knife_edge`;
- **alerts:** one per face and kind at its worst point, with value, unit, location and message.

Results come from a tessellation within the stated chord, except where the report marks a draft
as exact. Each triangle is judged at its centroid. Thickness is a single normal ray: at rims and
corners it measures through the adjacent wall.

**Not modelled:** a parting-line computation, side actions, rolling-ball thickness.

## Draft, Accessibility and Minimum Radius analyses

Fusion's core Inspect tools for moulded parts, as read-only analyses of one body. Each report
has a summary, per-face results and per-triangle arrays for colouring the model in the app.

- **`draft_analysis`** (`min_draft_deg` *rule* `DA`): signed draft `asin(n·pull)` per face and
  triangle, classed positive / negative / insufficient (exact for planes and coaxial
  cylinders/cones).
- **`accessibility`**: triangles reachable by a straight ray along the direction (`both`
  directions: either half of a two-plate mould), split into accessible / blocked / facing away /
  vertical.
- **`minimum_radius`** (`min_radius` *rule* `NR`): exact radii of cylinders, a mesh estimate for
  other curved faces; faces below the threshold flagged.

As for Design Advice, results come from a tessellation within the stated chord except where
marked exact.

## Example

`examples/plastic_enclosure_workflow.json` (`rcad-cli run`, replayed by `cargo xtask check`)
builds an 80 × 50 enclosure from scratch:
- a shelled base and lid with an ABS rule on both;
- an outer lip and groove;
- four screw bosses with gussets mating counterbored lid bosses;
- a drafted web to the floor;
- a drafted rest;
- two snap hooks from the lid catching windows in the base walls;
- Design Advice on the base.

It saves `examples/plastic_enclosure.rcad.json`. Order matters as in Fusion: the lip comes right
after the shells, because the lip's band is joined to the rim, and once drafted features have
made the body non-prismatic that join can fail.

## Evidence

**Feature tests** (`crates/rcad-commands/tests/it/plastics/`). Volumes are checked against
closed forms to 1e-6 relative, with point probes:
- bosses: plain, drafted frustums, through holes, gusset ribs (slab integral), a mating boss
  with clearance hole and counterbore;
- ribs and webs: plain, drafted, a web network to the next face;
- hooks: perpendicular, parallel stand-off, groove and loop volumes;
- lips: inner and outer bands, lip and groove;
- rests: plain and drafted, both opened through the floor.

**Rules, persistence and references** (same directory):
- rule binding and propagation, all-or-nothing validation, preset materials;
- references to boss faces that follow edits, and broken references;
- redefinition and undo;
- save and reload at v10, and a missing plugin.

**Independent validation:** `tools/check_plastics.sh` exports to STEP:
- bosses with gussets and with tip fillets;
- a drafted rest;
- a lip and groove;
- a snap-fit groove;
- a drafted box;
- a web along an arc;
- the enclosure example.

FreeCAD (OpenCascade) reads each of the 11 bodies as one valid solid. Volumes agree with the
closed forms within 2e-9 relative, and with R-CAD's own for the enclosure.

**More command tests** (same directory): boss tip fillets against the cap-loop oracle, rib to
next, ribs/webs along arcs (annular sectors), rounded rests, component rules, core shell rule
binding, and drafts of a box against the prismatoid volume (1e-9).

**Unit tests:**
- `crates/rcad-plugin-plastics/tests/advice.rs`: analytic meshes for thickness, draft (exact
  2°), undercuts, variation, knife edges and refusals, plus a 19,600-triangle performance check.
- `crates/rcad-plugin-plastics/tests/analyses.rs`: draft classes and signs, accessibility with
  slots and voids, exact and estimated radii, refusals, 20k-triangle timings.
- `crates/rcad-plugin-plastics/src`: manifest and rule templates.
