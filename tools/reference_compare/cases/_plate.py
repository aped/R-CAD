"""Shared set-up for the boss cases: a 60 x 40 x 4 mm plate with a copy of the
ABS plastic rule, and a sketch of two points 15 mm either side of the centre
on its top face. Reals are in the API's internal units (cm, radians)."""

import adsk.core
import adsk.fusion

V = adsk.core.ValueInput


def plate_with_points():
    app = adsk.core.Application.get()
    design = adsk.fusion.Design.cast(app.activeProduct)
    root = design.rootComponent
    sketch = root.sketches.add(root.xYConstructionPlane)
    sketch.sketchCurves.sketchLines.addCenterPointRectangle(
        adsk.core.Point3D.create(0, 0, 0), adsk.core.Point3D.create(3, 2, 0)
    )
    plate = root.features.extrudeFeatures.addSimple(
        sketch.profiles.item(0), V.createByReal(0.4), adsk.fusion.FeatureOperations.NewBodyFeatureOperation
    )
    plate.bodies.item(0).name = "Plate"
    rule = design.designPlasticRules.addByCopy(design.libraryPlasticRules.itemByName("ABS"), "Plate")
    rule.thickness.expression = "2 mm"
    rule.draftAngle.expression = "0 deg"
    root.activePlasticRule = rule
    pts_sketch = root.sketches.add(plate.endFaces.item(0))
    points = adsk.core.ObjectCollection.create()
    for x in (-1.5, 1.5):
        points.add(pts_sketch.sketchPoints.add(adsk.core.Point3D.create(x, 0, 0)))
    return root, points


def boss(root, points, **side_members):
    """Bosses 8 mm high, 6 mm across with a 2.5 mm pilot hole 6 mm deep, at
    `points`; `side_members` are extra BossFeatureSideInput members."""
    boss_in = root.features.bossFeatures.createInput()
    boss_in.setPositionBySketchPoints(points)
    side = boss_in.createSideInput()
    side.setSimple(V.createByString("6 mm"), V.createByString("2.5 mm"))
    side.draftAngle = V.createByReal(0)
    side.holeExtentType = adsk.fusion.BossHoleExtentTypes.BossBlindDepth
    side.holeDepth = V.createByReal(0.6)
    for name, value in side_members.items():
        setattr(side, name, value)
    boss_in.side1 = side
    boss_in.offset = V.createByReal(0.8)
    return root.features.bossFeatures.add(boss_in)
