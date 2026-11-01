"""An add-in with one command, "Plastic box": a box shelled open at the top,
given a copy of a plastic rule preset, with screw bosses along both long
walls and an optional web along the middle.

Run headless with:

    rcad-cli addin compat/scripting/examples/addins/PlasticBoxAddin --list
    rcad-cli addin compat/scripting/examples/addins/PlasticBoxAddin \
        --input width="100 mm" --input bosses=3 --out box.rcad.json

Values read from the inputs are in the API's internal units (cm, radians).
"""

import traceback

import adsk.core
import adsk.fusion

CMD_ID = "PlasticBoxCommand"
PANEL_ID = "SolidScriptsAddinsPanel"
WORKSPACE_ID = "FusionSolidEnvironment"

# Boss geometry (cm): 6 mm across, 2.5 mm pilot holes 6 mm deep, tops 5 mm
# below the rim, centres 5 mm in from the inner walls.
BOSS_DIAMETER = 0.6
HOLE_DIAMETER = 0.25
HOLE_DEPTH = 0.6
BOSS_TOP_BELOW_RIM = 0.5
BOSS_INSET = 0.5

PRESETS = ("ABS", "PC", "PP", "PA66")

_app = None
_ui = None
# Handlers must stay referenced for as long as the add-in runs.
_handlers = []


def _values(inputs):
    """The command's inputs as plain values (cm and counts)."""
    return {
        "width": inputs.itemById("width").value,
        "depth": inputs.itemById("depth").value,
        "height": inputs.itemById("height").value,
        "wall": inputs.itemById("wall").value,
        "wall_expr": inputs.itemById("wall").expression,
        "bosses": inputs.itemById("bosses").value,
        "web": inputs.itemById("web").value,
        "rule": inputs.itemById("rule").selectedItem.name,
    }


def _problems(v):
    """Why the values cannot make a box, or an empty list."""
    out = []
    w, d, h, t = v["width"], v["depth"], v["height"], v["wall"]
    if min(w, d, h, t) <= 0:
        return ["every size must be positive"]
    if 2 * t >= min(w, d):
        out.append("the walls meet")
    if t >= h:
        out.append("the floor fills the box")
    if v["bosses"] > 0:
        if (w - 2 * t) / v["bosses"] < BOSS_DIAMETER + 0.2:
            out.append("the bosses are too close together")
        boss_height = h - t - BOSS_TOP_BELOW_RIM
        if boss_height <= HOLE_DEPTH:
            out.append("the box is too shallow for the bosses")
        y = d / 2 - t - BOSS_INSET
        if v["web"] and y - BOSS_DIAMETER / 2 <= t + 0.1:
            out.append("the bosses would touch the web")
    return out


def _build(v):
    design = adsk.fusion.Design.cast(_app.activeProduct)
    root = design.rootComponent
    feats = root.features
    V = adsk.core.ValueInput
    P = adsk.core.Point3D
    w, d, h, t = v["width"], v["depth"], v["height"], v["wall"]

    sketch = root.sketches.add(root.xYConstructionPlane)
    sketch.sketchCurves.sketchLines.addCenterPointRectangle(P.create(0, 0, 0), P.create(w / 2, d / 2, 0))
    box = feats.extrudeFeatures.addSimple(
        sketch.profiles.item(0), V.createByReal(h), adsk.fusion.FeatureOperations.NewBodyFeatureOperation
    )
    body = box.bodies.item(0)
    body.name = "Box"
    rim = box.endFaces.item(0)
    shell_in = feats.shellFeatures.createInput([rim], False)
    shell_in.insideThickness = V.createByReal(t)
    feats.shellFeatures.add(shell_in)

    rule = design.designPlasticRules.addByCopy(design.libraryPlasticRules.itemByName(v["rule"]), "Box")
    rule.thickness.expression = v["wall_expr"]
    rule.draftAngle.expression = "0 deg"
    root.activePlasticRule = rule

    n = v["bosses"]
    if n > 0:
        plane_in = root.constructionPlanes.createInput()
        plane_in.setByOffset(rim, V.createByReal(-(h - t)))
        floor = root.sketches.add(root.constructionPlanes.add(plane_in))
        points = adsk.core.ObjectCollection.create()
        pitch = (w - 2 * t) / n
        y = d / 2 - t - BOSS_INSET
        for i in range(n):
            x = -(w / 2 - t) + pitch * (i + 0.5)
            for side in (-1, 1):
                points.add(floor.sketchPoints.add(P.create(x, side * y, 0)))
        boss_in = feats.bossFeatures.createInput()
        boss_in.setPositionBySketchPoints(points)
        s1 = boss_in.createSideInput()
        s1.setSimple(V.createByReal(BOSS_DIAMETER), V.createByReal(HOLE_DIAMETER))
        s1.draftAngle = V.createByReal(0)
        s1.holeExtentType = adsk.fusion.BossHoleExtentTypes.BossBlindDepth
        s1.holeDepth = V.createByReal(HOLE_DEPTH)
        boss_in.side1 = s1
        boss_in.offset = V.createByReal(h - t - BOSS_TOP_BELOW_RIM)
        feats.bossFeatures.add(boss_in)

    if v["web"]:
        web_sketch = root.sketches.add(rim)
        line = web_sketch.sketchCurves.sketchLines.addByTwoPoints(
            P.create(-(w / 2 - t / 2), 0, 0), P.create(w / 2 - t / 2, 0, 0)
        )
        web_in = feats.webFeatures.createInput([line], V.createByReal(t), body)
        web_in.isFlipped = True
        web_in.setToNextExtent()
        feats.webFeatures.add(web_in)


def _summary(inputs):
    v = _values(inputs)
    um = _app.activeProduct.unitsManager
    size = " x ".join(um.formatInternalValue(v[k], "mm", False) for k in ("width", "depth", "height"))
    return "%s mm, %d bosses" % (size, 2 * v["bosses"])


class _Created(adsk.core.CommandCreatedEventHandler):
    def notify(self, args):
        try:
            cmd = args.command
            inputs = cmd.commandInputs
            V = adsk.core.ValueInput
            inputs.addValueInput("width", "Width", "mm", V.createByString("80 mm"))
            inputs.addValueInput("depth", "Depth", "mm", V.createByString("50 mm"))
            inputs.addValueInput("height", "Height", "mm", V.createByString("25 mm"))
            inputs.addValueInput("wall", "Wall", "mm", V.createByReal(0.2))
            inputs.addIntegerSpinnerCommandInput("bosses", "Bosses per side", 0, 6, 1, 2)
            inputs.addBoolValueInput("web", "Add web", True, "", True)
            rule = inputs.addDropDownCommandInput(
                "rule", "Rule preset", adsk.core.DropDownStyles.TextListDropDownStyle
            )
            for name in PRESETS:
                rule.listItems.add(name, name == "ABS")
            inputs.addTextBoxCommandInput("summary", "Size", _summary(inputs), 1, True)

            for event, handler in (
                (cmd.execute, _Execute()),
                (cmd.executePreview, _Execute()),
                (cmd.inputChanged, _Changed()),
                (cmd.validateInputs, _Validate()),
                (cmd.destroy, _Destroy()),
            ):
                event.add(handler)
                _handlers.append(handler)
        except:  # noqa: E722 - shown to the user, as add-ins do
            _ui.messageBox("Failed:\n{}".format(traceback.format_exc()))


class _Execute(adsk.core.CommandEventHandler):
    def notify(self, args):
        try:
            _build(_values(args.command.commandInputs))
        except:  # noqa: E722
            _ui.messageBox("Failed:\n{}".format(traceback.format_exc()))


class _Changed(adsk.core.InputChangedEventHandler):
    def notify(self, args):
        if args.input.id != "summary":
            args.inputs.itemById("summary").text = _summary(args.inputs)


class _Validate(adsk.core.ValidateInputsEventHandler):
    def notify(self, args):
        args.areInputsValid = not _problems(_values(args.inputs))


class _Destroy(adsk.core.CommandEventHandler):
    def notify(self, args):
        pass


def run(context):
    global _app, _ui
    try:
        _app = adsk.core.Application.get()
        _ui = _app.userInterface
        cmd_def = _ui.commandDefinitions.itemById(CMD_ID)
        if not cmd_def:
            cmd_def = _ui.commandDefinitions.addButtonDefinition(
                CMD_ID, "Plastic box", "A shelled box with bosses and a web"
            )
        handler = _Created()
        cmd_def.commandCreated.add(handler)
        _handlers.append(handler)
        panel = _ui.workspaces.itemById(WORKSPACE_ID).toolbarPanels.itemById(PANEL_ID)
        control = panel.controls.addCommand(cmd_def)
        control.isPromoted = True
    except:  # noqa: E722
        if _ui:
            _ui.messageBox("Failed:\n{}".format(traceback.format_exc()))


def stop(context):
    try:
        panel = _ui.workspaces.itemById(WORKSPACE_ID).toolbarPanels.itemById(PANEL_ID)
        control = panel.controls.itemById(CMD_ID)
        if control:
            control.deleteMe()
        cmd_def = _ui.commandDefinitions.itemById(CMD_ID)
        if cmd_def:
            cmd_def.deleteMe()
    except:  # noqa: E722
        if _ui:
            _ui.messageBox("Failed:\n{}".format(traceback.format_exc()))
