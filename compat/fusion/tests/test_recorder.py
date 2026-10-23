"""Unit tests for the recorder behind R-CAD's subset of the Fusion add-in API.

    python3 -m unittest discover -s compat/fusion/tests

(also run by `cargo test -p rcad-cli`, crates/rcad-cli/tests/fusion_script.rs).
"""

import math
import os
import sys
import unittest

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))

import adsk  # noqa: E402
import adsk.core  # noqa: E402
import adsk.fusion  # noqa: E402

V = adsk.core.ValueInput
P = adsk.core.Point3D
F = adsk.fusion.FeatureOperations


def fresh():
    adsk.reset()
    app = adsk.core.Application.get()
    design = adsk.fusion.Design.cast(app.activeProduct)
    return design, design.rootComponent


def commands():
    return [s["command"] for s in adsk.recorder().finish()["steps"]]


def steps():
    return adsk.recorder().finish()["steps"]


def block(root, w=2.0, h=1.0, d=V.createByReal(1.0)):
    """A w x h cm rectangle on XY extruded by d."""
    sk = root.sketches.add(root.xYConstructionPlane)
    sk.sketchCurves.sketchLines.addTwoPointRectangle(P.create(0, 0, 0), P.create(w, h, 0))
    return sk, root.features.extrudeFeatures.addSimple(sk.profiles.item(0), d, F.NewBodyFeatureOperation)


class Units(unittest.TestCase):
    def test_reals_are_centimetres_and_radians(self):
        design, root = fresh()
        block(root, d=V.createByReal(1.25))
        cmds = commands()
        sk = cmds[1]["sketch"]
        self.assertEqual([(p["x"], p["y"]) for p in sk["points"]], [(0, 0), (20, 0), (20, 10), (0, 10)])
        self.assertEqual(cmds[2], {"cmd": "extrude", "sketch": 1, "distance": 12.5})

    def test_strings_carry_their_units(self):
        for text, mm in (("0.5 in", 12.7), ("3 cm", 30.0), ("7", 7.0), ("4mm", 4.0), ("0.01 m", 10.0)):
            design, root = fresh()
            block(root, d=V.createByString(text))
            self.assertAlmostEqual(commands()[2]["distance"], mm, places=12, msg=text)

    def test_negative_distance_reverses(self):
        design, root = fresh()
        block(root, d=V.createByString("-4 mm"))
        c = commands()[2]
        self.assertEqual((c["distance"], c.get("reverse")), (4.0, True))

    def test_angles_in_plastic_rules(self):
        design, root = fresh()
        rule = design.designPlasticRules.addByCopy(design.libraryPlasticRules.itemByName("pp"), "R")
        rule.draftAngle.value = math.radians(2)
        rule.thickness.value = 0.25
        rule.nominalRadius.expression = "Thickness / 4"
        self.assertAlmostEqual(float(rule._values["DA"]), 2.0, places=12)
        self.assertEqual(rule._values["T"], "2.5 mm")
        self.assertEqual(rule._values["NR"], "T / 4")
        self.assertAlmostEqual(rule.nominalRadius.value, 0.0625, places=12)
        self.assertAlmostEqual(rule.draftAngle.value, math.radians(2), places=12)


class Ids(unittest.TestCase):
    def test_features_and_bodies_are_numbered_in_order(self):
        design, root = fresh()
        design.userParameters.add("w", V.createByString("20 mm"), "mm", "width")
        _, ext = block(root)
        sk2 = root.sketches.add(ext.endFaces.item(0))
        sk2.sketchCurves.sketchCircles.addByCenterRadius(P.create(1, 0.5, 0), 0.2)
        ext2 = root.features.extrudeFeatures.addSimple(
            sk2.profiles.item(0), V.createByReal(0.3), F.JoinFeatureOperation
        )
        _, ext3 = block(root)
        st = steps()
        cmds = [s["command"]["cmd"] for s in st]
        self.assertEqual(
            cmds,
            ["new_document", "set_user_parameter", "create_sketch", "extrude", "create_sketch",
             "extrude", "create_sketch", "extrude"],
        )
        self.assertEqual([s.get("feature") for s in st], [None, None, 1, 2, 3, 4, 5, 6])
        self.assertEqual([s.get("body") for s in st], [None] * 3 + [1, None, None, None, 2])
        self.assertEqual(st[4]["command"]["plane"], {"Face": {"feature": 2, "role": "ExtrudeEndCap"}})
        self.assertEqual(st[5]["command"]["target"], 1)
        self.assertEqual(ext3.bodies.item(0).id, 2)
        self.assertIs(ext2.bodies.item(0), ext.bodies.item(0))

    def test_face_roles(self):
        design, root = fresh()
        _, ext = block(root)
        self.assertEqual(ext.startFaces.item(0)._ref(), {"feature": 2, "role": "ExtrudeStartCap"})
        self.assertEqual(
            [f._role for f in ext.sideFaces],
            [{"ExtrudeSide": {"entity": e}} for e in (1, 2, 3, 4)],
        )

    def test_offset_planes_add_up(self):
        design, root = fresh()
        _, ext = block(root)
        pi = root.constructionPlanes.createInput()
        pi.setByOffset(ext.endFaces.item(0), V.createByReal(-0.5))
        p1 = root.constructionPlanes.add(pi)
        pi = root.constructionPlanes.createInput()
        pi.setByOffset(p1, V.createByString("2 mm"))
        p2 = root.constructionPlanes.add(pi)
        sk = root.sketches.add(p2)
        sk.sketchPoints.add(P.create(0, 0, 0))
        c = commands()[-1]
        self.assertEqual(
            c["plane"],
            {"Offset": {"base": {"Face": {"feature": 2, "role": "ExtrudeEndCap"}}, "distance": -3.0}},
        )

    def test_rectangle_corners_are_shared(self):
        design, root = fresh()
        sk = root.sketches.add(root.xYConstructionPlane)
        sk.sketchCurves.sketchLines.addCenterPointRectangle(P.create(1, 1, 0), P.create(2, 1.5, 0))
        sk.sketchPoints.add(P.create(1, 1, 0))
        sk.sketchPoints.add(P.create(0, 0.5, 0))  # on a corner, but standalone
        s = commands()[-1]["sketch"]
        self.assertEqual(len(s["points"]), 6)
        self.assertEqual(
            [e["Line"]["a"] for e in s["entities"]] + [s["entities"][-1]["Line"]["b"]], [0, 1, 2, 3, 0]
        )


class Expressions(unittest.TestCase):
    def test_parameter_expressions_are_bound(self):
        design, root = fresh()
        p = design.userParameters.add("thick", V.createByReal(0.8), "mm", "")
        self.assertAlmostEqual(p.value, 0.8)
        design.userParameters.add("wide", V.createByString("2"), "in", "")
        sk, ext = block(root, d=V.createByString("thick * 2"))
        cmds = commands()
        self.assertEqual(cmds[1]["expression"], "8 mm")
        self.assertEqual(cmds[2]["expression"], "50.8 mm")
        self.assertEqual(cmds[4]["distance"], 16.0)
        self.assertEqual(
            cmds[5], {"cmd": "set_expression", "feature": 2, "name": "distance", "expression": "thick * 2"}
        )

    def test_negated_expressions_from_a_face_cut(self):
        design, root = fresh()
        design.userParameters.add("t", V.createByReal(1.0), "mm", "")
        _, ext = block(root, d=V.createByString("t"))
        sk = root.sketches.add(ext.endFaces.item(0))
        sk.sketchCurves.sketchCircles.addByCenterRadius(P.create(1, 0.5, 0), 0.2)
        ci = root.features.extrudeFeatures.createInput(sk.profiles.item(0), F.CutFeatureOperation)
        ci.setDistanceExtent(False, V.createByString("-t / 2"))
        root.features.extrudeFeatures.add(ci)
        cmds = commands()
        cut = [c for c in cmds if c.get("op") == "cut"][0]
        # Into the material from a face: R-CAD's default direction.
        self.assertEqual((cut["distance"], cut.get("reverse")), (5.0, None))
        self.assertEqual(cmds[-1]["expression"], "-(- t / 2)")

    def test_unknown_parameter_is_refused(self):
        design, root = fresh()
        with self.assertRaises(adsk.NotSupported):
            block(root, d=V.createByString("missing + 1"))


class Features(unittest.TestCase):
    def test_shell_and_fillet(self):
        design, root = fresh()
        _, ext = block(root)
        faces = adsk.core.ObjectCollection.create()
        faces.add(ext.endFaces.item(0))
        si = root.features.shellFeatures.createInput(faces, False)
        si.insideThickness = V.createByReal(0.15)
        root.features.shellFeatures.add(si)
        fi = root.features.filletFeatures.createInput()
        fi.edgeSetInputs.addConstantRadiusEdgeSet([ext.startFaces.item(0)], V.createByString("1 mm"), True)
        root.features.filletFeatures.add(fi)
        cmds = commands()
        self.assertEqual(
            cmds[3], {"cmd": "shell_face", "face": {"feature": 2, "role": "ExtrudeEndCap"}, "thickness": 1.5}
        )
        self.assertEqual(
            cmds[4],
            {"cmd": "fillet_loop", "face": {"feature": 2, "role": "ExtrudeStartCap"}, "radius": 1.0, "holes": True},
        )

    def test_single_edges_are_not_supported(self):
        design, root = fresh()
        _, ext = block(root)
        with self.assertRaisesRegex(adsk.NotSupported, "single B-rep edges"):
            ext.endFaces.item(0).edges.item(0)

    def test_boss_maps_to_plastics_parameters(self):
        design, root = fresh()
        _, ext = block(root)
        sk = root.sketches.add(ext.endFaces.item(0))
        a = sk.sketchPoints.add(P.create(0.5, 0.5, 0))
        sk.sketchPoints.add(P.create(1.5, 0.5, 0))
        bi = root.features.bossFeatures.createInput()
        bi.setPositionBySketchPoints(a)  # one of two points: a selection sketch
        side = bi.createSideInput()
        side.setSimple(V.createByReal(0.6), V.createByString("2 mm"))
        side.draftAngle = V.createByReal(math.radians(1))
        side.holeExtentType = adsk.fusion.BossHoleExtentTypes.BossHoleThrough
        side.ribCount = V.createByReal(4)
        side.tipRadius = V.createByReal(0)
        bi.side1 = side
        bi.offset = V.createByReal(1.0)
        root.features.bossFeatures.add(bi)
        st = steps()
        # The selection sketch (3) and the boss (4); the full points sketch is
        # emitted when the recording ends (5).
        boss = st[4]
        self.assertEqual(boss["feature"], 4)
        self.assertEqual((st[3]["feature"], st[5]["feature"]), (3, 5))
        self.assertEqual(st[3]["command"]["sketch"]["points"], [{"id": 0, "x": 5.0, "y": 5.0}])
        self.assertEqual(len(st[5]["command"]["sketch"]["points"]), 2)
        c = boss["command"]
        self.assertEqual(c["inputs"], {"sketch": {"kind": "sketch", "sketch": 3}})
        self.assertEqual(c["choices"], {"hole": "through"})
        self.assertEqual(c["params"]["diameter"], 6.0)
        self.assertEqual(c["params"]["hole_diameter"], 2.0)
        self.assertAlmostEqual(c["params"]["draft_deg"], 1.0, places=12)
        self.assertEqual(c["params"]["rib_count"], 4)
        self.assertEqual(
            boss["resolve"]["boss"],
            {"sketch": 3, "body": 1, "points": [[5.0, 5.0]], "offset": 10.0, "hole_full": False, "rib_offset": None},
        )

    def test_unmapped_boss_members_name_themselves(self):
        design, root = fresh()
        bi = root.features.bossFeatures.createInput()
        side = bi.createSideInput()
        with self.assertRaisesRegex(adsk.NotSupported, "BossFeatureSideInput.setCounterbore"):
            side.setCounterbore(V.createByReal(1), V.createByReal(1), V.createByReal(1), V.createByReal(1))
        with self.assertRaisesRegex(adsk.NotSupported, "BossFeatureSideInput.alignmentDraftAngle"):
            side.alignmentDraftAngle = V.createByReal(0.1)
        with self.assertRaisesRegex(adsk.NotSupported, "BossFeatureSideInput.rootRadius"):
            side.rootRadius = V.createByReal(0.1)
        with self.assertRaisesRegex(adsk.NotSupported, "BossFeatureInput.side2"):
            bi.side2 = bi.createSideInput()

    def test_boss_alignment_step_and_tip_radius(self):
        design, root = fresh()
        _, ext = block(root)
        sk = root.sketches.add(ext.endFaces.item(0))
        a = sk.sketchPoints.add(P.create(0.5, 0.5, 0))
        bi = root.features.bossFeatures.createInput()
        bi.setPositionBySketchPoints(a)
        side = bi.createSideInput()
        side.setSimple(V.createByReal(0.6), V.createByString("2 mm"))
        side.alignmentType = adsk.fusion.BossAlignmentTypes.BossAlignStepOut
        side.alignmentDiameter = V.createByReal(0.45)
        side.alignmentDepth = V.createByString("1 mm")
        side.tipRadius = V.createByReal(0.05)
        bi.side1 = side
        bi.offset = V.createByReal(1.0)
        root.features.bossFeatures.add(bi)
        c = [x for x in steps() if x["command"].get("feature") == "boss"][0]["command"]
        self.assertEqual(c["choices"]["step"], "out")
        self.assertAlmostEqual(c["params"]["step_diameter"], 4.5, places=12)
        self.assertAlmostEqual(c["params"]["step_depth"], 1.0, places=12)
        self.assertAlmostEqual(c["params"]["tip_radius"], 0.5, places=12)

    def test_rib_and_web(self):
        design, root = fresh()
        _, ext = block(root)
        body = ext.bodies.item(0)
        sk = root.sketches.add(root.xZConstructionPlane)
        line = sk.sketchCurves.sketchLines.addByTwoPoints(P.create(0, 0.5, 0), P.create(2, 0.5, 0))
        ri = root.features.ribFeatures.createInput([line], V.createByReal(0.1), body)
        ri.isFlipped = True
        with self.assertRaisesRegex(adsk.NotSupported, "RibFeatureInput.setToNextExtent"):
            ri.setToNextExtent()
        ri.setDistanceExtent(V.createByReal(0.4))
        root.features.ribFeatures.add(ri)
        wi = root.features.webFeatures.createInput(sk, V.createByString("1 mm"), body)
        wi.setToNextExtent()
        root.features.webFeatures.add(wi)
        rib, web = [c for c in commands() if c["cmd"] == "plugin_feature"]
        self.assertEqual(rib["feature"], "rib")
        self.assertEqual(rib["choices"], {"side": "left", "thickness_side": "symmetric"})
        self.assertEqual(rib["params"], {"thickness": 1.0, "depth": 4.0})
        self.assertEqual(rib["inputs"]["body"], {"kind": "body", "body": 1})
        self.assertEqual(web["choices"]["extent"], "to_next")
        self.assertEqual(web["inputs"]["sketch"]["sketch"], 3)

    def test_rules_are_assigned_to_the_component_bodies(self):
        design, root = fresh()
        rule = design.designPlasticRules.addByCopy(design.libraryPlasticRules.itemByName("ABS"), "Case")
        root.activePlasticRule = rule
        block(root)
        block(root)
        cmds = commands()
        self.assertEqual(
            cmds[-1],
            {
                "cmd": "set_plugin_data",
                "plugin": "plastics",
                "data": {
                    "rules": [{"name": "Case", "material": "abs", "values": {"DA": "1", "T": "2 mm"}}],
                    "bodies": {"1": "Case", "2": "Case"},
                },
            },
        )
        with self.assertRaises(adsk.NotSupported):
            root.activePlasticRule = design.libraryPlasticRules.itemByName("ABS")


class Subset(unittest.TestCase):
    def test_unknown_members_raise_not_supported(self):
        design, root = fresh()
        with self.assertRaises(adsk.NotSupported) as c:
            root.features.loftFeatures
        self.assertEqual(
            str(c.exception), "adsk.fusion.Features.loftFeatures is not in R-CAD's Fusion API subset"
        )
        with self.assertRaisesRegex(adsk.NotSupported, r"adsk\.fusion\.SweepFeatures is not"):
            adsk.fusion.SweepFeatures
        with self.assertRaisesRegex(adsk.NotSupported, r"adsk\.core\.Vector3D is not"):
            adsk.core.Vector3D.create(1, 0, 0)
        with self.assertRaisesRegex(adsk.NotSupported, r"Component\.isVisible"):
            root.isVisible = False
        self.assertEqual(len(adsk.recorder().not_supported), 4)

    def test_cast(self):
        design, root = fresh()
        self.assertIs(adsk.fusion.Design.cast(design), design)
        self.assertIsNone(adsk.fusion.Design.cast(root))


if __name__ == "__main__":
    unittest.main()
