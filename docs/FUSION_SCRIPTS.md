# Fusion-style Python scripts

R-CAD runs Python scripts written against a subset of the Fusion add-in API surface
(`adsk.core`, `adsk.fusion`). The scripts are the kind that build a part: sketches, extrusions,
a shell, fillets, user parameters, and the plastic features and rules.

The `adsk` package in `compat/fusion/adsk/` is an independent, pure-Python (standard library)
implementation. It was written from the public API reference on help.autodesk.com. No Autodesk
code, samples or documentation text are used. R-CAD is not affiliated with or endorsed by
Autodesk.

The package does not model anything itself. It records each supported call as R-CAD commands
(docs/COMMANDS.md); the CLI then runs them in a session as `rcad-cli run` does. A member
outside the subset raises `adsk.NotSupported` naming it:

```text
adsk.fusion.Features.loftFeatures is not in R-CAD's Fusion API subset
```

## Running a script

```sh
rcad-cli fusion-script SCRIPT.py [--out DOC.rcad.json] [--commands OUT.json]
```

- The script runs under `python3 -I` (isolated mode; set `RCAD_PYTHON` to choose another
  interpreter). The script itself is not modified.
- The `adsk` package and a small bootstrap are built into `rcad-cli`. They are written to a
  private temporary directory for the run.
- The bootstrap imports the script, calls `run(context)` (Fusion's entry point), then
  `stop(context)` if the script defines it. The script's directory is on `sys.path`, so it can
  import modules beside it.
- `print` output and message boxes go to standard error.
- Standard output carries one JSON line per executed step, as in `rcad-cli run`.
- Each recorded step is checked against the feature id (and new body id) the recorder
  predicted, and the run stops at the first refused step.
- `--out` saves the document if every step succeeded.
- `--commands` writes the executed commands as a plain JSON array that `rcad-cli run`
  replays. Boss heights are filled in (see Boss below).

**Exit status:**

| status | meaning |
|---|---|
| 0 | success |
| 1 | a Python error (with its traceback), an unsupported member (with the script line), a script that reported a failure, or a refused step |
| 2 | the script cannot be read, Python cannot be started, or an output cannot be written |
| 64 | usage |

**Failures the script catches.** Scripts often wrap `run` in `try`/`except` and show the
traceback in `ui.messageBox`. A `NotSupported` raised inside the script fails the run even when
the script catches it, because the model would be incomplete. A message box shown while an
exception is being handled is also taken as the script reporting a failure.

## Units

| API value | R-CAD |
|---|---|
| `ValueInput.createByReal(x)`, length | `x` cm → `10·x` mm |
| `ValueInput.createByReal(x)`, angle | `x` rad → degrees |
| `Point3D` coordinates, `addByCenterRadius` radius | cm → mm |
| `ValueInput.createByString("5 mm")` | units `mm`, `cm`, `m`, `in`, `ft`, `"`, `deg`, `rad`, `°`; a plain number is in mm (lengths) or degrees (angles), the design's default units |
| `createByString("thick * 2")` (a parameter expression) | evaluated for the command's value and bound to the feature parameter (`set_expression`), so editing the parameter regenerates the feature |

Expressions accept `+ - * /`, parentheses, `min`, `max` and `abs`, as R-CAD's do
(docs/PARAMETERS.md). Angle units inside expressions are dropped, since R-CAD angles are plain
degrees. `rad` and non-R-CAD length units are refused inside expressions; they are accepted in
plain quantities.

## Supported subset

Everything not listed raises `NotSupported`. `cast`, `classType`, `objectType` and `isValid`
exist on every object; collections have `count`, `item(i)` and iteration.

### adsk.core

| Class.member | Mapping |
|---|---|
| `Application.get()`, `.activeProduct`, `.userInterface`, `.activeDocument`, `.documents.add(DocumentTypes.FusionDesignDocumentType)` | one design per run (`new_document`); `documents.add` only before anything is modelled |
| `UserInterface.messageBox(text, title, ...)` | printed; returns `DialogResults.DialogOK` |
| `Point3D.create(x, y, z)`, `.x/.y/.z`, `copy`, `asArray`, `distanceTo`, `isEqualTo` | cm |
| `ValueInput.createByReal`, `.createByString`, `.realValue`, `.stringValue`, `.valueType` | see Units |
| `ObjectCollection.create`, `createWithArray`, `add`, `item`, `count`, `removeByIndex`, `removeByItem`, `clear`, `contains`, `find` | plain list; Python lists are accepted wherever the API takes arrays |
| `adsk.autoTerminate`, `adsk.terminate`, `adsk.doEvents` | accepted, no effect |
| `adsk.cam` | importable; every member is unsupported |

### adsk.fusion: design, planes, sketches

| Class.member | Mapping |
|---|---|
| `Design.cast`, `.rootComponent`, `.userParameters`, `.designPlasticRules`, `.libraryPlasticRules`, `.designType` | `designType` accepts `ParametricDesignType` only |
| `Component.xYConstructionPlane`, `.xZConstructionPlane`, `.yZConstructionPlane` | `WorldXY`, `WorldXZ`, `WorldYZ` |
| `Component.constructionPlanes.createInput()`, `ConstructionPlaneInput.setByOffset(planarEntity, ValueInput)`, `constructionPlanes.add(input)` | an `Offset` sketch plane on the world plane or face; offsets of offset planes add up. A parameter expression binds the sketch's `plane_offset` |
| `Component.sketches.add(planarEntity)` | `create_sketch` (`template: custom`) on a construction plane or an extrusion face (`Face` plane), emitted when a feature first uses it (later edits: `edit_sketch`) |
| `Sketch.sketchCurves.sketchLines.addByTwoPoints(p, q)` | `Line` entity; points are `Point3D` (sketch space, z = 0) or `SketchPoint`s |
| `sketchLines.addTwoPointRectangle(p, q)`, `.addCenterPointRectangle(c, corner)` | four lines with shared corners (`SketchLineList`) |
| `Sketch.sketchCurves.sketchCircles.addByCenterRadius(c, r)` | `Circle` with a `Radius` constraint |
| `Sketch.sketchPoints.add(p)` | a standalone point |
| `SketchLine.startSketchPoint/.endSketchPoint`, `SketchCircle.centerSketchPoint/.radius`, `SketchPoint.geometry`, `SketchCurve.isConstruction` | every point is fixed (`FixedPoint`), so sketches are fully constrained |
| `Sketch.name`, `.profiles`, `.referencePlane`, `.isComputeDeferred` | `profiles.item(0)` is the whole profile of the sketch |
| `UserParameters.add(name, ValueInput, units, comment)`, `.itemByName`; `UserParameter.expression`, `.value`, `.unit`, `.comment` | `set_user_parameter`; units `mm`, `cm`, `m`, `in` (R-CAD unit mm), `deg`, `rad` (deg), `""` (unitless). A plain number takes the parameter's units (`"2"` in `in` is `2 in`) |

### adsk.fusion: features

| Class.member | Mapping |
|---|---|
| `extrudeFeatures.addSimple(profile, distance, operation)` | `extrude` |
| `extrudeFeatures.createInput(profile, operation)`, `ExtrudeFeatureInput.setDistanceExtent(False, d)`, `.setOneSideExtent(DistanceExtentDefinition or ThroughAllExtentDefinition, direction)`, `.setAllExtent(direction)`, `.participantBodies`, `.operation`; `extrudeFeatures.add(input)` | `NewBodyFeatureOperation` → new body; `Join`/`Cut`/`Intersect` → `op` with `target`: the one participant body, else the body whose face holds the sketch, else the only body. A negative distance or `NegativeExtentDirection` → `reverse`. Through All: cuts only |
| `ExtrudeFeature.startFaces`, `.endFaces`, `.sideFaces`, `.faces`, `.bodies`, `.name` | `BRepFace`s with roles `ExtrudeStartCap`, `ExtrudeEndCap`, `ExtrudeSide{entity}` (one per profile line); `name` → `rename_feature` |
| `BRepBody.name`, `.parentComponent`; `BRepFace.body`, `.edges` | `name` → `rename_body` |
| `shellFeatures.createInput(ObjectCollection[one BRepFace], isTangentChain)`, `ShellFeatureInput.insideThickness`; `shellFeatures.add` | `shell_face` (open at that face); `outsideThickness` only 0 |
| `filletFeatures.createInput()`, `FilletFeatureInput.edgeSetInputs.addConstantRadiusEdgeSet(entities, radius, isTangentChain)` (or the older `FilletFeatureInput.addConstantRadiusEdgeSet`); `filletFeatures.add` | one face (or that face's `edges`) → `fillet_loop` with `holes: true`: every edge of the face, as when a face is selected for a fillet. Single edges raise `NotSupported` |

### adsk.fusion: plastic features

| Class.member | Mapping (plastics plugin, docs/PLASTICS.md) |
|---|---|
| `bossFeatures.createInput()`, `BossFeatureInput.setPositionBySketchPoints(points)`, `.createSideInput()`, `.side1`, `.offset`, `.participantBodies`; `bossFeatures.add` | `plugin_feature` `boss` on the points' sketch. When the points are not exactly the sketch's standalone points, a selection sketch on the same plane is added. The boss body is the participant body or the body whose face holds the sketch (directly or through an offset plane) |
| `BossFeatureSideInput.setSimple(diameter, holeDiameter)` / `.setBlank(diameter)` | `diameter`, `hole_diameter`; hole `pilot` / `none` |
| `.draftAngle`, `.holeDraftAngle`, `.holeDepth` | `draft_deg`, `hole_draft_deg`, `hole_depth` |
| `.holeExtentType` | `BossHoleThrough` → hole `through`; `BossBlindDepth` → pilot of `holeDepth`; `BossBlindFull` (or no type and no depth) → pilot the full boss height |
| `.ribCount`, `.ribLength`, `.ribThickness`, `.ribRotation`, `.ribTotalAngle`, `.ribOffset`, `.ribType` | `rib_count`, `rib_length` (from the axis), `rib_thickness`, `rib_angle_deg`, `rib_total_deg`, `rib_height` = boss height − `ribOffset`; `ribType` `BossRibShapeChamfer` (R-CAD's gusset profile) or `BossRibShapeNone` |
| `.tipRadius` | `tip_radius` (fillet of the shank top) |
| `.alignmentType` (`BossAlignFlat`, `BossAlignStepOut`, `BossAlignStepIn`), `.alignmentDiameter`, `.alignmentDepth` | `step` (none/out/in), `step_diameter`, `step_depth` |
| `.rootRadius`, `.holeStartRadius`, `.holeEndRadius`, `.ribRootRadius`, `.ribTipRadius`, `.ribDraftAngle`, `.ribOuterDraftAngle` | accepted only as 0 (not modelled) |
| `ribFeatures.createInput(curves, thickness, targetBody)`, `RibFeatureInput.setDistanceExtent(depth)`, `.thickness`, `.draftAngle`, `.isFlipped`, `.isSymmetric`, `.targetBody`; `ribFeatures.add` | `plugin_feature` `rib`: `thickness`, `depth`, `draft_deg`; side `right` (the curve direction × the sketch normal) or `left` when flipped; `thickness_side` `symmetric` / `one_side` |
| `webFeatures.createInput(curves, thickness, targetBody)`, `WebFeatureInput.setDistanceExtent(depth)`, `.setToNextExtent()`, `.thickness`, `.draftAngle`, `.isFlipped`, `.isSymmetric`, `.targetBody`; `webFeatures.add` | `plugin_feature` `web`: as the rib, with `direction` `normal` / `flip` and `extent` `distance` / `to_next` |

`curves` is a list or `ObjectCollection` of sketch lines from one sketch, or a `Sketch` (all of
its curves). A subset of a sketch's lines gets a selection sketch, as for bosses.

**Boss height.** In the API a boss is placed by its parting face: the position points plus
`offset`. R-CAD's boss stands on the face below its sketch and is `height` tall. The CLI
resolves the height when the step runs. For each position point it takes the nearest face of
the boss body at or below the point that faces along the sketch normal, and checks that there
is material just below that face and none just above it, under the point. The height is that
depth plus `offset`, and all points must give the same height. So with the points on the floor,
`offset` is the boss height. A boss with no face under a point is refused with the reason.

### Plastic rules

| Class.member | Mapping |
|---|---|
| `Design.libraryPlasticRules.itemByName(name)` | R-CAD's presets (`ABS`, `ASA`, `PC`, `PC_ABS`, `PP`, `PA66`, `POM`, `HDPE`; case-insensitive), read-only |
| `Design.designPlasticRules.addByCopy(rule, name)`, `.itemByName` | a design rule. Names must be identifiers, as for R-CAD rules |
| `PlasticRule.name`, `.material` (a material catalog key, e.g. `"abs"`, docs/MATERIALS.md), `.units`, `.isDefault`, `.isUsed` | |
| `PlasticRule.thickness`, `.draftAngle`, `.nominalRadius`, `.clearance`, `.revealHeight`, `.minimumThickness`, `.maximumThickness`, `.minimumDraftAngle`, `.knifeEdgeThreshold` → `PlasticRuleValue` with `.value` (cm / rad) and `.expression` | rule symbols `T`, `DA`, `NR`, `CL`, `RH`, `TMIN`, `TMAX`, `MDA`, `KE`; `Thickness` in an expression is `T` |
| `Component.activePlasticRule = rule` | **R-CAD extension**, by analogy with `activeSheetMetalRule`: the public API reference does not show how a script assigns a rule to a component. The rule applies to every body of the component |

The rules and assignments are stored with `set_plugin_data` before the next plastic feature
and at the end of the run, whenever they changed. As in R-CAD, features created before a rule
was stored keep their plain values.

## Not supported, and deviations

- **Components and occurrences:** only the root component; `occurrenceForCreation` and
  `creationOccurrence` must be `None`.
- **Sketch axes on the XZ and YZ planes:** sketch coordinates use R-CAD's plane axes
  (`WorldXZ`: x = X, y = Z, normal −Y; `WorldYZ`: x = Y, y = Z, normal +X). Fusion's own
  axes on those planes may differ, so scripts sketching there should be checked. Points on
  faces use R-CAD's face frames, which for a face parallel to XY are world X and Y.
- **Profiles:** `sketch.profiles` holds one profile, the whole sketch. Fusion splits sketches
  into regions; `item(i)` for `i > 0` returns `None`.
- **Coincident points:** line end points at the same coordinates are merged into one sketch
  point, so loops drawn from `Point3D`s close. Standalone points are never merged.
- **Cut and join from a face:** R-CAD cuts into the material and joins away from it. A
  negative distance from a face cut goes into the material, as in the API. A positive one
  is refused, since it removes nothing. A sketch on a world or offset plane that happens to
  contain a face of the target follows R-CAD's rule (docs/COMMANDS.md `extrude`).
- **Extrude:** symmetric and two-sided extents, taper angles, and extents to an object or
  face are not mapped. Only one profile per extrusion.
- **Shell:** a single removed face only. Shelling a closed body and outside thickness are not
  mapped.
- **Fillet:** whole faces only. Variable and chord-length fillets are not mapped, nor is `isG2`.
- **Boss:** `side2` (the mating boss) is not mapped, because R-CAD's mating boss needs the mate
  face (use the `mate` input of `plugin_feature` `boss`). Also not mapped: drafted alignment steps,
  counterbores and countersinks on side 1, fillet-shaped ribs, `setRibExtent`, rib cut size and
  chamfer angle, and `isDefaultDirection = False`. The boss offset is a plain value (not bound to
  a parameter).
- **Rib and web:** ribs take a distance (no to-next). `isThicknessFromTop`, `isExtended` (extend
  curves), fillet radii other than 0 and `draftPullDirection` are not mapped. Only straight
  lines are accepted.
- **Plastic rules:** `thicknessVariation` is not mapped: R-CAD's `TV` is a ratio. Library rules
  cannot be edited or added to. There is no per-component rule other than the root's.
- **Not in the subset at all:** revolve, loft, sweep, patterns, mirror, chamfer, holes,
  combine, snap fits, lips, rests, selections, the UI (commands, palettes, events), the data
  panel, CAM, and every member not listed above.

## Examples

- `compat/fusion/examples/plate_with_hole.py`: a 100 × 60 mm plate whose 8 mm thickness is a
  user parameter (`createByReal(0.8)`, i.e. cm), with a 15 mm hole cut down from the top face.
  Both extrusions are bound to the parameter.
- `compat/fusion/examples/plastic_box.py`: an 80 × 50 × 25 mm box shelled to 2 mm and given a
  copy of the ABS rule. It has four 15 mm bosses on the floor with 6 mm pilot holes, a web from
  the rim to the floor, and a rib across the cavity.

```sh
rcad-cli fusion-script compat/fusion/examples/plastic_box.py --out box.rcad.json --commands box.json
rcad-cli run box.json        # the same model from the recorded commands
```

## Tests

- `crates/rcad-cli/tests/fusion_script.rs` (`cargo test -p rcad-cli`; skipped with a note when
  `python3` is missing):
  - runs both examples through the binary and checks the saved volumes against hand-derived
    closed forms. The plate is 100·60·8 − π·7.5²·8. The box is the shell
    80·50·25 − 76·46·23, plus four bosses 4(π·3²·15 − π·1.25²·6), the web 46·2·23 and the
    rib 76·2·13 less the 2·2·13 where it crosses the web;
  - replays the recorded commands with `rcad-cli run`;
  - checks the cm→mm and inch conversion, an unsupported member (and one the script catches),
    Python errors, a refused step and the exit codes.
- `compat/fusion/tests/test_recorder.py` (`python3 -m unittest discover -s compat/fusion/tests`,
  also run by the Rust test): units, feature and body ids, face roles, offset planes,
  expressions and bindings, shell, fillet, boss, rib and web mappings, rules, and
  `NotSupported` messages.

These tests check R-CAD's mapping against the public API reference. They are not a comparison
with Fusion: no script has been run in Fusion for these results.
