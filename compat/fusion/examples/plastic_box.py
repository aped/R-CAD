"""An 80 x 50 x 25 mm plastic box: shelled to 2 mm walls, with a plastic
rule, four screw bosses on the floor, a web from the rim down to the floor
and a rib across the cavity.

Run with:  rcad-cli fusion-script compat/fusion/examples/plastic_box.py --out box.rcad.json

Reals are in the API's internal units (cm, radians).
"""

import adsk.core
import adsk.fusion

V = adsk.core.ValueInput


def run(context):
    app = adsk.core.Application.get()
    design = adsk.fusion.Design.cast(app.activeProduct)
    root = design.rootComponent
    feats = root.features

    # Box: 80 x 50 mm, 25 mm high, open at the top with 2 mm walls.
    base = root.sketches.add(root.xYConstructionPlane)
    base.sketchCurves.sketchLines.addCenterPointRectangle(
        adsk.core.Point3D.create(0, 0, 0), adsk.core.Point3D.create(4, 2.5, 0)
    )
    box = feats.extrudeFeatures.addSimple(
        base.profiles.item(0), V.createByReal(2.5), adsk.fusion.FeatureOperations.NewBodyFeatureOperation
    )
    body = box.bodies.item(0)
    body.name = "Housing"
    rim = box.endFaces.item(0)
    faces = adsk.core.ObjectCollection.create()
    faces.add(rim)
    shell = feats.shellFeatures.createInput(faces, False)
    shell.insideThickness = V.createByReal(0.2)
    feats.shellFeatures.add(shell)

    # Plastic rule: a copy of the ABS preset, 2 mm walls, no draft.
    abs_rule = design.libraryPlasticRules.itemByName("ABS")
    rule = design.designPlasticRules.addByCopy(abs_rule, "Housing")
    rule.thickness.expression = "2 mm"
    rule.draftAngle.expression = "0 deg"
    root.activePlasticRule = rule

    # Bosses: points on the floor (2 mm above the bottom, 23 mm below the rim).
    plane_in = root.constructionPlanes.createInput()
    plane_in.setByOffset(rim, V.createByString("-23 mm"))
    floor = root.constructionPlanes.add(plane_in)
    pts_sketch = root.sketches.add(floor)
    points = adsk.core.ObjectCollection.create()
    for x, y in ((-3, -1.5), (3, -1.5), (3, 1.5), (-3, 1.5)):
        points.add(pts_sketch.sketchPoints.add(adsk.core.Point3D.create(x, y, 0)))
    boss_in = feats.bossFeatures.createInput()
    boss_in.setPositionBySketchPoints(points)
    side = boss_in.createSideInput()
    side.setSimple(V.createByString("6 mm"), V.createByString("2.5 mm"))
    side.draftAngle = V.createByReal(0)
    side.holeExtentType = adsk.fusion.BossHoleExtentTypes.BossBlindDepth
    side.holeDepth = V.createByReal(0.6)
    boss_in.side1 = side
    # The boss top is 15 mm above its position points.
    boss_in.offset = V.createByReal(1.5)
    feats.bossFeatures.add(boss_in)

    # Web: a line on the rim across the box (x = 0), down to the floor.
    web_sketch = root.sketches.add(rim)
    web_line = web_sketch.sketchCurves.sketchLines.addByTwoPoints(
        adsk.core.Point3D.create(0, -2.4, 0), adsk.core.Point3D.create(0, 2.4, 0)
    )
    web_in = feats.webFeatures.createInput([web_line], V.createByReal(0.2), body)
    web_in.isFlipped = True
    web_in.setToNextExtent()
    feats.webFeatures.add(web_in)

    # Rib: a line 15 mm up in the XZ plane, wall to wall, growing 13.5 mm
    # down into the floor.
    rib_sketch = root.sketches.add(root.xZConstructionPlane)
    rib_line = rib_sketch.sketchCurves.sketchLines.addByTwoPoints(
        adsk.core.Point3D.create(-3.9, 1.5, 0), adsk.core.Point3D.create(3.9, 1.5, 0)
    )
    rib_in = feats.ribFeatures.createInput([rib_line], V.createByString("2 mm"), body)
    rib_in.draftAngle = V.createByReal(0)
    rib_in.setDistanceExtent(V.createByReal(1.35))
    feats.ribFeatures.add(rib_in)
