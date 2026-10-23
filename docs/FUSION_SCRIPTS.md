# Fusion-style Python scripts and add-ins

R-CAD runs Python scripts written against a subset of the Fusion add-in API surface
(`adsk.core`, `adsk.fusion`). The scripts are the kind that build a part: sketches, extrusions,
a shell, fillets, user parameters, and the plastic features and rules. Add-ins (a folder with a
manifest, whose commands have dialogs) run headless with `rcad-cli fusion-addin`; see
[Add-ins](#add-ins).

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
| `Application.log(message, level, type)`, `LogLevels`, `LogTypes` | printed, like a message box |
| Command definitions, events, commands, command inputs, workspaces and toolbars | see [Add-ins](#add-ins) |
| `adsk.cam` | importable; every member is unsupported |

### adsk.fusion: design, planes, sketches

| Class.member | Mapping |
|---|---|
| `Design.cast`, `.rootComponent`, `.userParameters`, `.designPlasticRules`, `.libraryPlasticRules`, `.designType` | `designType` accepts `ParametricDesignType` only |
| `Design.unitsManager` / `.fusionUnitsManager`: `.defaultLengthUnits`, `.internalUnits`, `evaluateExpression(expr, units)`, `isValidExpression`, `formatInternalValue(value, units, showUnits)`, `convert(value, from, to)` | the design is in mm; values in cm / rad; a plain number is in `units` |
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
| `.rootRadius` | `root_radius` (exact concave root fillet; 0 when not set) |
| `.alignmentType` (`BossAlignFlat`, `BossAlignStepOut`, `BossAlignStepIn`), `.alignmentDiameter`, `.alignmentDepth` | `step` (none/out/in), `step_diameter`, `step_depth` |
| `.holeStartRadius`, `.holeEndRadius`, `.ribRootRadius`, `.ribTipRadius`, `.ribDraftAngle`, `.ribOuterDraftAngle` | accepted only as 0 (not modelled). Every boss is recorded with `root_radius` 0, so the rule's `NR` does not add a root fillet |
| `ribFeatures.createInput(curves, thickness, targetBody)`, `RibFeatureInput.setDistanceExtent(depth)`, `.setToNextExtent()`, `.thickness`, `.draftAngle`, `.isFlipped`, `.isSymmetric`, `.targetBody`; `ribFeatures.add` | `plugin_feature` `rib`: `thickness`, `depth`, `draft_deg`; side `right` (the curve direction × the sketch normal) or `left` when flipped; `thickness_side` `symmetric` / `one_side`; `extent` `distance` / `to_next` |
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
- **Rib and web:** ribs and webs take a distance or to-next. `isThicknessFromTop`, `isExtended` (extend
  curves), fillet radii other than 0 and `draftPullDirection` are not mapped. Only straight
  lines are accepted.
- **Plastic rules:** `thicknessVariation` is not mapped: R-CAD's `TV` is a ratio. Library rules
  cannot be edited or added to. There is no per-component rule other than the root's.
- **Not in the subset at all:** revolve, loft, sweep, patterns, mirror, chamfer, holes,
  combine, snap fits, lips, rests, the interactive UI (palettes, HTML, custom graphics,
  keyboard and mouse events, active selections), the data panel, CAM, and every member not
  listed above. Commands, their events and inputs exist for add-ins run headless (below).

## Add-ins

```sh
rcad-cli fusion-addin ADDIN_DIR --list
rcad-cli fusion-addin ADDIN_DIR [--command ID] [--input ID=VALUE ...] [--base SCRIPT.py]
                                [--out DOC.rcad.json] [--commands OUT.json]
```

An add-in is a folder `Name/` with `Name.manifest` and `Name.py`. Fusion keeps add-ins loaded
and shows their commands as buttons; each command opens a dialog of inputs. R-CAD has no such
user interface for them, so the CLI plays the user's part for one command and then runs the
recorded R-CAD commands exactly as `fusion-script` does (same output, `--out`, `--commands`
and checks).

**Manifest.** The manifest is JSON. `type` must be `"addin"`. A `"script"` manifest is refused
with a pointer to `fusion-script`, and so is a manifest with no type or with invalid JSON.
`id`, `author`, `version`, `autodeskProduct` and `supportedOS` must be strings and
`runOnStartup` a boolean when present. `description` may be a string or the localized object
form. These values are shown by `--list` and otherwise not used. If no `<folder
name>.manifest` exists, a single other `.manifest` in the folder is used. A folder with a
native library and no `.py` is refused, since C++ add-ins are not supported.

**Exit status** as for `fusion-script`. 1 also covers an input value the add-in cannot take,
inputs its `validateInputs` handler rejects, and a command that cannot be chosen. 2 also
covers a folder, manifest or `.py` that cannot be read and a manifest that is not an add-in's.
`--list` together with `--command`, `--input`, `--out` or `--commands` is a usage error (64).

**What runs, in order:**

1. `Name.py` is imported as a module of a package rooted at the add-in folder, so its
   relative imports (`from . import commands`, `from .lib import helpers`) work. The folder is
   also on `sys.path`. Then `run(context)` is called with `{"IsApplicationStartup": False}`.
2. The command is chosen: `--command ID`, else the last one the add-in started with
   `CommandDefinition.execute()` in `run`, else the only registered definition. Otherwise the
   run is refused and the ids are listed.
3. `commandCreated` fires with a new `Command`. Its handler adds the inputs and connects the
   command's events.
4. Each `--input ID=VALUE` is set on its input (searched in groups and tabs too), in the order
   given, and `inputChanged` fires for it as if the user had edited it. Hidden or disabled
   inputs cannot be set. An unknown id or a value the input cannot take refuses the run with
   the reason (exit 1).
5. `validateInputs` fires. `areInputsValid = False` refuses the run, as the dialog's OK button
   would stay disabled. Selection inputs with fewer selections than their minimum are refused
   too.
6. `executePreview`, if connected, fires in a forked child process. Whatever it records is
   discarded with the process, as a preview is discarded in Fusion. If the handler sets
   `args.isValidResult = True`, the preview is fired again in the main process and kept, and
   `execute` is skipped. A preview that fails is reported on standard error and discarded.
   Without `fork` (not Linux/Unix) previews are skipped.
7. `execute` fires. `args.executeFailed = True` fails the run with `executeFailedMessage`.
8. `destroy` fires with `CompletedTerminationReason`, then the add-in's
   `stop({"IsApplicationClosing": False})`.

As for scripts, an exception a handler does not catch fails the run, and so does a
`NotSupported` it catches or a message box shown while handling an exception.

**`--list`** runs `run`, fires `commandCreated` for every definition, calls `stop`, and prints
one JSON document: the manifest fields and, per command, `id`, `name`, `tooltip`,
`resourceFolder`, `placements` (the toolbar panels it was added to) and `inputs` with their
types and defaults, e.g.
`{"id": "width", "type": "ValueCommandInput", "unitType": "mm", "expression": "80 mm", "value": 8.0}`.

**Input values on the command line:**

| Input (`CommandInputs.add…`) | `--input ID=…` | What the add-in reads |
|---|---|---|
| `addValueInput(id, name, unitType, ValueInput)`, `addDistanceValueCommandInput(id, name, ValueInput)` (unit type mm; `setManipulator` accepted) | an expression as in `ValueInput.createByString` (`5 mm`, `0.25 in`, `30 deg`, `thick * 2`); a plain number is in the input's unit type | `.value` in internal units (cm, rad), `.expression` as typed (a plain number gets the unit: `2` → `2 in`), `.isValidExpression`, `.unitType` |
| `addBoolValueInput(id, name, isCheckBox, resourceFolder, initialValue)` | `true`/`false` (also `1`/`0`, `yes`/`no`, `on`/`off`) | `.value` |
| `addIntegerSpinnerCommandInput(id, name, min, max, spinStep, initialValue)` | an integer within min..max | `.value`, `.minimumValue`, `.maximumValue`, `.spinStep` |
| `addDropDownCommandInput(id, name, DropDownStyles…)`, `.listItems.add(name, isSelected, icon, beforeIndex)` | an item name; for `CheckBoxDropDownStyle` a comma-separated list | `.selectedItem.name`, `ListItem.isSelected`, `.index` |
| `addStringValueInput(id, name, initialValue)` | any text | `.value` |
| `addTextBoxCommandInput(id, name, formattedText, numRows, isReadOnly)` | any text (not when read-only) | `.text`, `.formattedText` |
| `addSelectionInput(id, name, prompt)`, `.addSelectionFilter(f)`, `.clearSelectionFilter()`, `.setSelectionLimits(min, max)` (default 1..1, max 0: no limit), `.addSelection(entity)`, `.clearSelection()` | `face:<n or last>:<endFaces, startFaces, sideFaces or faces>[:<i>]` or `body:<n or last>`; repeat `--input` for more selections | `.selectionCount`, `.selection(i).entity` (a `BRepFace` or `BRepBody`) |
| `addGroupCommandInput(id, name)`, `addTabCommandInput(id, name)` | not settable | `.children` (a `CommandInputs`) |

Every input has `id`, `name`, `isVisible`, `isEnabled`, `parentCommand`,
`parentCommandInput`, `commandInputs` and `deleteMe()`. Tooltips, `isFullWidth` and similar
dialog settings are stored and have no effect. Other input kinds (tables, sliders, float
spinners, images, button rows, ...) raise `NotSupported` by name.

**Selections.** Without a user interface there is nothing to click, so a selection names a
face of the features recorded so far. `n` counts the features created through the API in
this run from 1: those of the `--base` script, then those of the add-in's own `run`. Sketches
and planes are not counted. `i` is the face collection's `item(i)` index, 0 by default.
`body:n` is the n-th body. Only extrusions have addressable faces. Filters `Faces` and
`PlanarFaces` take faces, `Bodies` and `SolidBodies` take bodies, no filter takes either, and
other filters (edges, sketch curves, vertices, ...) take nothing. `--base SCRIPT.py` runs a
Fusion-style script first, in the same design. It stands in for the document open when the
command is started. An add-in may also pre-fill a selection input from its own geometry with
`addSelection`.

**Supporting objects:**

| Class.member | Mapping |
|---|---|
| `UserInterface.commandDefinitions`: `addButtonDefinition(id, name, tooltip, resourceFolder)`, `itemById`, `count`, `item` | a duplicate id raises `RuntimeError`; `itemById` of an unknown id is `None` |
| `CommandDefinition.id`, `.name`, `.tooltip`, `.resourceFolder`, `.toolClipFilename`, `.controlDefinition` (`isEnabled`, `isVisible`, `name`), `.commandCreated`, `.execute()`, `.deleteMe()`, `.isValid` | `execute()` requests the command (step 2) |
| `Command.commandInputs`, `.parentCommandDefinition`; events `.execute`, `.executePreview`, `.inputChanged`, `.validateInputs`, `.destroy`, `.activate`, `.deactivate` (the last two never fire); `okButtonText`, `cancelButtonText`, `isOKButtonVisible`, `isRepeatable`, `isExecutedWhenPreEmpted`, `isPositionDependent`, `helpFile`, `setDialogInitialSize`, `setDialogMinimumSize` | dialog settings are stored, no effect |
| Events: `add(handler)`, `remove(handler)`, `name`, `sender` | `add` needs an object with `notify(args)` and returns `False` for a handler already added. The `handler` annotation of `add` names the handler class in `adsk.core`, for helper code that subclasses it |
| `EventHandler`, `CommandCreatedEventHandler`, `CommandEventHandler`, `InputChangedEventHandler`, `ValidateInputsEventHandler` | plain base classes; subclasses keep any state |
| `CommandCreatedEventArgs.command`; `CommandEventArgs.command`, `.executeFailed`, `.executeFailedMessage`, `.isValidResult`, `.terminationReason`; `InputChangedEventArgs.input`, `.inputs`; `ValidateInputsEventArgs.inputs`, `.areInputsValid`; `.firingEvent` on all | |
| `UserInterface.workspaces.itemById(id)`, `.allToolbarPanels`, `.allToolbarTabs`; `Workspace.toolbarPanels`, `.toolbarTabs`; `ToolbarTabs`/`ToolbarPanels` `itemById`, `add`; `ToolbarPanel.controls`: `addCommand(def, positionID, isBefore)`, `addSeparator`, `addDropDown(...).controls`, `itemById`; `CommandControl.isPromoted`, `.isPromotedByDefault`, `.isVisible`, `.deleteMe()` | placement is recorded and shown by `--list`. Fusion's built-in workspace, tab and panel ids are not modelled, so `itemById` of any id returns a placeholder instead of `None` |
| `CommandTerminationReason`, `DropDownStyles` | named values (R-CAD's own numbers) |

**Limits.** There is no interactive dialog, and no session: one command runs once per call,
and the result is the recorded model. Previews are discarded (except with `isValidResult`).
Palettes, HTML, custom graphics, mouse and keyboard events, `activeSelections`,
`CommandDefinition.execute(input)` with an argument, check-box definitions, and the add-in
being started at application startup are not emulated. Uncaught handler exceptions fail
the run, where Fusion may only log them. Only Python add-ins run; C++ add-ins are refused.

## Examples

- `compat/fusion/examples/addins/PlasticBoxAddin/`: an add-in with one command,
  `PlasticBoxCommand` ("Plastic box"). Its inputs are width, depth and height (80 × 50 × 25
  mm), wall (2 mm), bosses per side (spinner 0..6, 2), "Add web" (true) and a rule preset
  drop-down (ABS, PC, PP, PA66). A read-only text box shows the size and is updated from
  `inputChanged`. `validateInputs` rejects walls that meet and bosses that do not fit, and
  the preview builds the same model (discarded). `execute` builds the box, shells it open at
  the top, copies the chosen preset with the wall as its thickness, and adds bosses 6 mm
  across with 2.5 mm pilot holes 6 mm deep. They stand on the floor 5 mm in from the long
  walls, with tops 5 mm below the rim. With the web on, a wall-thick web runs along the middle
  from rim to floor.

```sh
rcad-cli fusion-addin compat/fusion/examples/addins/PlasticBoxAddin --list
rcad-cli fusion-addin compat/fusion/examples/addins/PlasticBoxAddin \
    --input width="100 mm" --input bosses=3 --input web=false --input rule=PP --out box.rcad.json
```

Scripts:

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
  - runs `PlasticBoxAddin` through `fusion-addin`. `--list` gives the command, its inputs
    and their defaults. With the default inputs the volume is
    80·50·25 − 76·46·23 + 4(π·3²·18 − π·1.25²·6) + 76·2·23 (shell, bosses, web). With
    `width=100 mm depth=6 cm wall=2.5 bosses=3 web=false rule=PP` it is
    100·60·25 − 95·55·22.5 + 6(π·3²·17.5 − π·1.25²·6), with the `pp` material in the
    rule. Inputs the add-in's `validateInputs` rejects and values an input cannot take are
    refused. Non-add-in, untyped and invalid manifests are refused, and so is an add-in
    using an unsupported member (in `execute` and in `commandCreated`). A selection names
    the top face of a `--base` script's 30 × 30 × 10 mm block, and the add-in joins a
    10 × 10 × 6.35 mm block onto it;
- `compat/fusion/tests/test_recorder.py` (`python3 -m unittest discover -s compat/fusion/tests`,
  also run by the Rust test): units, feature and body ids, face roles, offset planes,
  expressions and bindings, shell, fillet, boss, rib and web mappings, rules, and
  `NotSupported` messages.
- `compat/fusion/tests/test_events.py`: events and handlers, command definitions, toolbar
  placement, every input kind's value and expression semantics, selections and filters, the
  units manager, and the add-in driver. The driver tests cover the event order, the
  discarded and the kept preview, validation, `executeFailed`, command choice, `--base`
  selections, relative imports and unsupported members.

These tests check R-CAD's mapping against the public API reference. They are not a comparison
with Fusion: no script has been run in Fusion for these results.
