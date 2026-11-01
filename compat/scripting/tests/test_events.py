"""Unit tests for the add-in command model (events, command definitions,
commands, command inputs) and the headless add-in driver ``adsk._addin``.

    python3 -m unittest discover -s compat/scripting/tests

(also run by `cargo test -p rcad-cli`).
"""

import math
import os
import sys
import tempfile
import textwrap
import unittest

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))

import adsk  # noqa: E402
import adsk.core  # noqa: E402
import adsk.fusion  # noqa: E402
from adsk import _addin  # noqa: E402

C = adsk.core
V = adsk.core.ValueInput


def fresh():
    adsk.reset()
    app = C.Application.get()
    return app, app.userInterface


def command():
    _, ui = fresh()
    d = ui.commandDefinitions.addButtonDefinition("cmd", "Cmd", "tip")
    return C.Command(d)


class Recorder(C.CommandEventHandler):
    def __init__(self, log, tag):
        super().__init__()
        self.log = log
        self.tag = tag

    def notify(self, args):
        self.log.append((self.tag, args))


class Events(unittest.TestCase):
    def test_add_remove_and_fire(self):
        cmd = command()
        log = []
        a, b = Recorder(log, "a"), Recorder(log, "b")
        self.assertTrue(cmd.execute.add(a))
        self.assertTrue(cmd.execute.add(b))
        self.assertFalse(cmd.execute.add(a), "the same handler is added once")
        cmd.execute._fire(C.CommandEventArgs(cmd.execute, cmd))
        self.assertEqual([t for t, _ in log], ["a", "b"])
        self.assertIs(log[0][1].command, cmd)
        self.assertIs(log[0][1].firingEvent, cmd.execute)
        self.assertTrue(cmd.execute.remove(a))
        self.assertFalse(cmd.execute.remove(a))
        log.clear()
        cmd.execute._fire(C.CommandEventArgs(cmd.execute, cmd))
        self.assertEqual([t for t, _ in log], ["b"])

    def test_handlers_need_notify(self):
        cmd = command()
        with self.assertRaises(TypeError):
            cmd.execute.add(object())

    def test_handler_type_is_named_by_add(self):
        # Helper code can find the handler class to subclass from the event.
        for event, handler in (
            (C.CommandCreatedEvent, C.CommandCreatedEventHandler),
            (C.CommandEvent, C.CommandEventHandler),
            (C.InputChangedEvent, C.InputChangedEventHandler),
            (C.ValidateInputsEvent, C.ValidateInputsEventHandler),
        ):
            name = event.add.__annotations__["handler"]
            self.assertIs(getattr(sys.modules[event.__module__], name), handler)
            self.assertEqual(event.classType(), "adsk::core::" + event.__name__)

    def test_handlers_keep_state(self):
        class H(C.CommandCreatedEventHandler):
            def __init__(self):
                super().__init__()
                self.seen = None

            def notify(self, args):
                self.seen = args.command

        _, ui = fresh()
        d = ui.commandDefinitions.addButtonDefinition("x", "X", "")
        h = H()
        d.commandCreated.add(h)
        cmd = C.Command(d)
        d.commandCreated._fire(C.CommandCreatedEventArgs(d.commandCreated, cmd))
        self.assertIs(h.seen, cmd)
        self.assertIs(cmd.parentCommandDefinition, d)


class Definitions(unittest.TestCase):
    def test_add_find_delete(self):
        _, ui = fresh()
        defs = ui.commandDefinitions
        self.assertIsNone(defs.itemById("a"))
        d = defs.addButtonDefinition("a", "A", "tip", "./resources")
        self.assertEqual((d.id, d.name, d.tooltip, d.resourceFolder), ("a", "A", "tip", "./resources"))
        self.assertIs(defs.itemById("a"), d)
        self.assertEqual(defs.count, 1)
        with self.assertRaises(RuntimeError):
            defs.addButtonDefinition("a", "again", "")
        self.assertTrue(d.deleteMe())
        self.assertIsNone(defs.itemById("a"))
        self.assertFalse(d.isValid)

    def test_execute_requests_the_command(self):
        _, ui = fresh()
        d = ui.commandDefinitions.addButtonDefinition("a", "A", "")
        self.assertTrue(d.execute())
        self.assertEqual(ui.commandDefinitions._requested, [d])

    def test_toolbar_placement_is_recorded(self):
        _, ui = fresh()
        d = ui.commandDefinitions.addButtonDefinition("a", "A", "")
        panel = ui.workspaces.itemById("W").toolbarPanels.itemById("P")
        c = panel.controls.addCommand(d)
        c.isPromoted = True
        self.assertIs(panel.controls.itemById("a"), c)
        tab_panel = ui.allToolbarTabs.itemById("T").toolbarPanels.add("Mine", "Mine")
        tab_panel.controls.addDropDown("More", "").controls.addCommand(d)
        self.assertEqual(
            C._placements(ui, d),
            [
                {"tab": "T", "panel": "Mine", "dropDown": "", "promoted": False},
                {"workspace": "W", "panel": "P", "promoted": True},
            ],
        )
        self.assertTrue(c.deleteMe())
        self.assertIsNone(panel.controls.itemById("a"))

    def test_unknown_members_are_named(self):
        cmd = command()
        with self.assertRaises(adsk.NotSupported) as e:
            cmd.commandInputs.addTableCommandInput("t", "T", 2, "1:1")
        self.assertIn("adsk.core.CommandInputs.addTableCommandInput is not in", str(e.exception))
        with self.assertRaises(adsk.NotSupported) as e:
            C.Application.get().userInterface.palettes
        self.assertIn("adsk.core.UserInterface.palettes", str(e.exception))


class Inputs(unittest.TestCase):
    def test_value_input_from_a_real_is_shown_in_its_units(self):
        inputs = command().commandInputs
        w = inputs.addValueInput("w", "W", "mm", V.createByReal(0.2))
        self.assertEqual((w.value, w.expression, w.unitType), (0.2, "2 mm", "mm"))
        w.value = 2.54
        self.assertEqual(w.expression, "25.4 mm")
        a = inputs.addValueInput("a", "A", "deg", V.createByReal(math.pi / 6))
        self.assertEqual(a.expression, "30 deg")

    def test_value_input_expressions(self):
        inputs = command().commandInputs
        w = inputs.addValueInput("w", "W", "in", V.createByString("0.25 in"))
        self.assertAlmostEqual(w.value, 0.635, places=12)
        w.expression = "2"  # a plain number is in the input's units
        self.assertEqual(w.expression, "2 in")
        self.assertAlmostEqual(w.value, 5.08, places=12)
        w.expression = "10 mm"
        self.assertAlmostEqual(w.value, 1.0, places=12)
        self.assertTrue(w.isValidExpression)
        w.expression = "nonsense +"
        self.assertFalse(w.isValidExpression)
        self.assertAlmostEqual(w.value, 1.0, places=12, msg="the value is kept")
        self.assertEqual(adsk.recorder().not_supported, [], "an invalid value is not an API gap")
        a = inputs.addValueInput("a", "A", "deg", V.createByString("30"))
        self.assertAlmostEqual(a.value, math.pi / 6, places=12)
        n = inputs.addValueInput("n", "N", "", V.createByReal(3))
        self.assertEqual((n.value, n.expression), (3.0, "3"))

    def test_value_input_with_a_parameter(self):
        adsk.reset()
        design = adsk.fusion.Design.cast(C.Application.get().activeProduct)
        design.userParameters.add("thick", V.createByString("3 mm"), "mm", "")
        d = C.Application.get().userInterface.commandDefinitions.addButtonDefinition("c", "C", "")
        w = C.Command(d).commandInputs.addValueInput("w", "W", "mm", V.createByString("thick * 2"))
        self.assertAlmostEqual(w.value, 0.6, places=12)
        self.assertEqual(w.expression, "thick * 2")

    def test_unsupported_unit_types(self):
        inputs = command().commandInputs
        with self.assertRaises(adsk.NotSupported):
            inputs.addValueInput("x", "X", "mm * mm", V.createByReal(1))

    def test_bool_integer_string_text(self):
        inputs = command().commandInputs
        b = inputs.addBoolValueInput("b", "B", True, "", True)
        self.assertIs(b.value, True)
        b._set_text("false")
        self.assertIs(b.value, False)
        with self.assertRaises(C.InvalidValue):
            b._set_text("maybe")
        n = inputs.addIntegerSpinnerCommandInput("n", "N", 1, 5, 1, 2)
        self.assertEqual((n.value, n.minimumValue, n.maximumValue, n.spinStep), (2, 1, 5, 1))
        n._set_text("5")
        self.assertEqual(n.value, 5)
        with self.assertRaises(C.InvalidValue):
            n._set_text("6")
        with self.assertRaises(C.InvalidValue):
            n._set_text("2.5")
        s = inputs.addStringValueInput("s", "S", "hello")
        s._set_text("world")
        self.assertEqual(s.value, "world")
        t = inputs.addTextBoxCommandInput("t", "T", "info", 2, True)
        self.assertEqual((t.text, t.numRows, t.isReadOnly), ("info", 2, True))
        with self.assertRaises(C.InvalidValue):
            t._set_text("x")

    def test_drop_downs(self):
        inputs = command().commandInputs
        dd = inputs.addDropDownCommandInput("d", "D", C.DropDownStyles.TextListDropDownStyle)
        dd.listItems.add("one", True)
        two = dd.listItems.add("two", False)
        self.assertEqual(dd.selectedItem.name, "one")
        two.isSelected = True
        self.assertEqual([i.isSelected for i in dd.listItems], [False, True], "single selection")
        dd._set_text("one")
        self.assertEqual(dd.selectedItem.name, "one")
        with self.assertRaises(C.InvalidValue) as e:
            dd._set_text("three")
        self.assertIn("'one', 'two'", str(e.exception))
        multi = inputs.addDropDownCommandInput("m", "M", C.DropDownStyles.CheckBoxDropDownStyle)
        for n in ("a", "b", "c"):
            multi.listItems.add(n, n == "a")
        multi._set_text("b, c")
        self.assertEqual([i.isSelected for i in multi.listItems], [False, True, True])

    def test_groups_and_lookup(self):
        cmd = command()
        g = cmd.commandInputs.addGroupCommandInput("g", "G")
        inner = g.children.addBoolValueInput("inner", "Inner", True)
        self.assertIs(cmd.commandInputs.itemById("inner"), inner, "itemById finds nested inputs")
        self.assertIs(inner.parentCommandInput, g)
        self.assertIs(inner.parentCommand, cmd)
        with self.assertRaises(RuntimeError):
            cmd.commandInputs.addBoolValueInput("inner", "Again", True)
        self.assertEqual(cmd.commandInputs.count, 1)

    def test_selections(self):
        adsk.reset()
        root = adsk.fusion.Design.cast(C.Application.get().activeProduct).rootComponent
        sk = root.sketches.add(root.xYConstructionPlane)
        sk.sketchCurves.sketchLines.addTwoPointRectangle(C.Point3D.create(0, 0, 0), C.Point3D.create(1, 1, 0))
        ext = root.features.extrudeFeatures.addSimple(
            sk.profiles.item(0), V.createByReal(1), adsk.fusion.FeatureOperations.NewBodyFeatureOperation
        )
        d = C.Application.get().userInterface.commandDefinitions.addButtonDefinition("c", "C", "")
        sel = C.Command(d).commandInputs.addSelectionInput("s", "S", "Pick a face")
        sel.addSelectionFilter("PlanarFaces")
        self.assertFalse(sel.addSelection(ext.bodies.item(0)), "a body does not pass a face filter")
        self.assertTrue(sel.addSelection(ext.endFaces.item(0)))
        self.assertFalse(sel.addSelection(ext.startFaces.item(0)), "the default limit is one")
        self.assertEqual(sel.selectionCount, 1)
        self.assertIs(sel.selection(0).entity, ext.endFaces.item(0))
        sel.setSelectionLimits(0, 0)
        self.assertTrue(sel.addSelection(ext.startFaces.item(0)))
        # Command-line selections name features in creation order.
        self.assertIs(_addin.selection_entity("face:last:endFaces"), ext.endFaces.item(0))
        self.assertIs(_addin.selection_entity("face:1:sideFaces:2"), ext.sideFaces.item(2))
        self.assertIs(_addin.selection_entity("body:1"), ext.bodies.item(0))
        for bad in ("face:2:endFaces", "face:1:topFaces", "face:1:sideFaces:9", "edge:1", "face:x:endFaces"):
            with self.assertRaises(_addin.AddinError, msg=bad):
                _addin.selection_entity(bad)


class UnitsManager(unittest.TestCase):
    def test_units_manager(self):
        adsk.reset()
        um = adsk.fusion.Design.cast(C.Application.get().activeProduct).unitsManager
        self.assertEqual(um.defaultLengthUnits, "mm")
        self.assertAlmostEqual(um.evaluateExpression("1 in", "mm"), 2.54, places=12)
        self.assertAlmostEqual(um.evaluateExpression("5", "mm"), 0.5, places=12)
        self.assertEqual(um.formatInternalValue(2.5, "mm", True), "25 mm")
        self.assertAlmostEqual(um.convert(1, "in", "mm"), 25.4, places=12)
        self.assertFalse(um.isValidExpression("2 deg", "mm"))


# ---------------------------------------------------------------------------
# The driver, on add-ins written to a temporary folder

ADDIN = '''
import adsk.core, adsk.fusion, traceback
V = adsk.core.ValueInput
P = adsk.core.Point3D
_handlers = []
LOG = []


def build(size, height, op=adsk.fusion.FeatureOperations.NewBodyFeatureOperation, plane=None):
    root = adsk.core.Application.get().activeProduct.rootComponent
    sk = root.sketches.add(plane or root.xYConstructionPlane)
    sk.sketchCurves.sketchLines.addTwoPointRectangle(P.create(0, 0, 0), P.create(size, size, 0))
    return root.features.extrudeFeatures.addSimple(sk.profiles.item(0), V.createByReal(height), op)


class Execute(adsk.core.CommandEventHandler):
    def notify(self, args):
        LOG.append("execute")
        inputs = args.command.commandInputs
        build(inputs.itemById("size").value, 1)
        {execute_extra}


class Preview(adsk.core.CommandEventHandler):
    def notify(self, args):
        LOG.append("preview")
        build(5, 5)
        {preview_extra}


class Validate(adsk.core.ValidateInputsEventHandler):
    def notify(self, args):
        args.areInputsValid = args.inputs.itemById("size").value > 0


class Changed(adsk.core.InputChangedEventHandler):
    def notify(self, args):
        LOG.append("changed " + args.input.id)


class Destroy(adsk.core.CommandEventHandler):
    def notify(self, args):
        LOG.append("destroy %d" % args.terminationReason)


class Created(adsk.core.CommandCreatedEventHandler):
    def notify(self, args):
        cmd = args.command
        cmd.commandInputs.addValueInput("size", "Size", "mm", V.createByString("10 mm"))
        {created_extra}
        for ev, h in ((cmd.execute, Execute()), (cmd.executePreview, Preview()),
                      (cmd.validateInputs, Validate()), (cmd.inputChanged, Changed()),
                      (cmd.destroy, Destroy())):
            ev.add(h)
            _handlers.append(h)


def run(context):
    ui = adsk.core.Application.get().userInterface
    d = ui.commandDefinitions.addButtonDefinition("Cmd", "Command", "tip")
    h = Created()
    d.commandCreated.add(h)
    _handlers.append(h)
    {run_extra}


def stop(context):
    LOG.append("stop")
'''


def write_addin(folder, name="Test", execute_extra="pass", preview_extra="pass",
                created_extra="pass", run_extra="pass"):
    d = os.path.join(folder, name)
    os.makedirs(d, exist_ok=True)
    src = ADDIN
    for k, v in (("execute_extra", execute_extra), ("preview_extra", preview_extra),
                 ("created_extra", created_extra), ("run_extra", run_extra)):
        src = src.replace("{%s}" % k, v)
    with open(os.path.join(d, name + ".py"), "w") as f:
        f.write(textwrap.dedent(src))
    return d, os.path.join(d, name + ".py")


def module(name="Test"):
    return next(m for k, m in sys.modules.items() if k.endswith("." + name) and k.startswith("rcad_addin_"))


def cmds(result):
    return [s["command"]["cmd"] for s in result["steps"]]


class Driver(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)

    def run_addin(self, inputs=(), list_=False, command=None, base=None, **extra):
        d, py = write_addin(self.tmp.name, **extra)
        return _addin.execute(
            {"addin": d, "script": py, "list": list_, "command": command,
             "inputs": [list(i) for i in inputs], "base": base}
        )

    def test_list(self):
        r = self.run_addin(list_=True)
        self.assertNotIn("error", r)
        (c,) = r["listing"]
        self.assertEqual((c["id"], c["name"], c["tooltip"]), ("Cmd", "Command", "tip"))
        self.assertEqual(
            c["inputs"],
            [{"id": "size", "name": "Size", "type": "ValueCommandInput", "unitType": "mm",
              "expression": "10 mm", "value": 1.0}],
        )
        self.assertNotIn("execute", module().LOG)

    def test_the_command_flow(self):
        r = self.run_addin(inputs=[("size", "2 cm")])
        self.assertNotIn("error", r, r)
        self.assertEqual(module().LOG, ["changed size", "execute", "destroy 1", "stop"])
        # Only the execute geometry is recorded: the preview ran in a discarded process.
        self.assertEqual(cmds(r), ["new_document", "create_sketch", "extrude"])
        pts = r["steps"][1]["command"]["sketch"]["points"]
        self.assertEqual(max(p["x"] for p in pts), 20.0)
        self.assertEqual(r["addin"]["command"], "Cmd")
        # Without fork (Windows) previews are skipped (docs/SCRIPTING.md).
        self.assertEqual(r["addin"]["preview"], "discarded" if hasattr(os, "fork") else "skipped")
        self.assertEqual(r["addin"]["inputs"][0]["expression"], "2 cm")

    def test_a_valid_preview_result_is_kept(self):
        r = self.run_addin(preview_extra="args.isValidResult = True")
        self.assertNotIn("error", r, r)
        if not hasattr(os, "fork"):
            # Previews are skipped without fork: execute builds the model.
            self.assertEqual(r["addin"]["preview"], "skipped")
            self.assertIn("execute", module().LOG)
            return
        self.assertEqual(module().LOG, ["preview", "destroy 1", "stop"])
        self.assertEqual(r["steps"][2]["command"]["distance"], 50.0)
        self.assertEqual(r["addin"]["preview"], "kept")

    def test_inputs_the_validate_handler_rejects_are_refused(self):
        r = self.run_addin(inputs=[("size", "-1 mm")])
        self.assertEqual(r["error"]["kind"], "addin")
        self.assertIn("areInputsValid = False", r["error"]["message"])
        self.assertNotIn("execute", module().LOG)

    def test_input_errors(self):
        for inputs, text in (
            ([("nope", "1")], "has no input 'nope' (inputs: 'size')"),
            ([("size", "2 deg")], "input 'size'"),
            ([("size", "1"), ("size", "2")], "given twice"),
        ):
            r = self.run_addin(inputs=inputs)
            self.assertEqual(r["error"]["kind"], "addin", inputs)
            self.assertIn(text, r["error"]["message"])

    def test_execute_failed(self):
        r = self.run_addin(execute_extra="args.executeFailed = True; args.executeFailedMessage = 'no room'")
        self.assertEqual(r["error"]["kind"], "script")
        self.assertIn("no room", r["error"]["message"])

    def test_unsupported_member_in_a_handler(self):
        r = self.run_addin(execute_extra="adsk.core.Application.get().activeProduct.rootComponent.features.loftFeatures")
        self.assertEqual(r["error"]["kind"], "not_supported")
        self.assertIn("adsk.fusion.Features.loftFeatures", r["error"]["message"])

    def test_a_caught_unsupported_member_still_fails(self):
        r = self.run_addin(
            execute_extra=(
                "try:\n            inputs.addSliderCommandInput\n"
                "        except Exception:\n"
                "            adsk.core.Application.get().userInterface.messageBox(traceback.format_exc())"
            )
        )
        self.assertEqual(r["error"]["kind"], "not_supported")
        self.assertIn("addSliderCommandInput", r["error"]["message"])

    def test_choosing_a_command(self):
        extra = ("d2 = ui.commandDefinitions.addButtonDefinition('Other', 'Other', '')\n"
                 "    d2.commandCreated.add(h)")
        r = self.run_addin(run_extra=extra)
        self.assertIn("several commands", r["error"]["message"])
        r = self.run_addin(run_extra=extra, command="Other")
        self.assertNotIn("error", r, r)
        self.assertEqual(r["addin"]["command"], "Other")
        r = self.run_addin(run_extra=extra + "\n    d2.execute()")
        self.assertEqual(r["addin"]["command"], "Other", "the command run() asked for")
        r = self.run_addin(command="Missing")
        self.assertIn("no command 'Missing' (commands: 'Cmd')", r["error"]["message"])

    def test_selection_from_a_base_script(self):
        base = os.path.join(self.tmp.name, "base.py")
        with open(base, "w") as f:
            f.write(textwrap.dedent('''
                import adsk.core, adsk.fusion
                def run(context):
                    root = adsk.core.Application.get().activeProduct.rootComponent
                    sk = root.sketches.add(root.xYConstructionPlane)
                    sk.sketchCurves.sketchLines.addTwoPointRectangle(
                        adsk.core.Point3D.create(0, 0, 0), adsk.core.Point3D.create(3, 3, 0))
                    root.features.extrudeFeatures.addSimple(
                        sk.profiles.item(0), adsk.core.ValueInput.createByReal(1),
                        adsk.fusion.FeatureOperations.NewBodyFeatureOperation)
            '''))
        created = ("s = cmd.commandInputs.addSelectionInput('face', 'Face', 'Pick')\n"
                   "        s.addSelectionFilter('PlanarFaces')")
        execute = ("build(1, 0.5, adsk.fusion.FeatureOperations.JoinFeatureOperation, "
                   "inputs.itemById('face').selection(0).entity)")
        r = self.run_addin(created_extra=created, execute_extra=execute, base=base)
        self.assertIn("needs at least 1 selection", r["error"]["message"])
        r = self.run_addin(created_extra=created, execute_extra=execute, base=base,
                           inputs=[("face", "face:1:endFaces")])
        self.assertNotIn("error", r, r)
        join = r["steps"][-1]["command"]
        self.assertEqual(join["op"], "join")
        self.assertEqual(r["steps"][-2]["command"]["plane"], {"Face": {"feature": 2, "role": "ExtrudeEndCap"}})
        r = self.run_addin(created_extra=created, execute_extra=execute, base=base,
                           inputs=[("face", "body:1")])
        self.assertIn("does not pass the selection filters", r["error"]["message"])

    def test_relative_imports_from_the_addin_folder(self):
        d, py = write_addin(self.tmp.name, name="Rel", run_extra="from . import helper; LOG.append(helper.X)")
        with open(os.path.join(d, "helper.py"), "w") as f:
            f.write("X = 'helper loaded'\n")
        r = _addin.execute({"addin": d, "script": py, "list": True})
        self.assertNotIn("error", r, r)
        self.assertIn("helper loaded", module("Rel").LOG)


if __name__ == "__main__":
    unittest.main()
