"""An open 60 x 40 x 20 mm box with 2 mm walls and a 2 mm web across it from
the rim to the floor, with 1 mm root fillets.
Closed form: 60*40*20 - 56*36*18 + 36*2*18 + 2*36*(1 - pi/4) mm^3."""

import adsk.core
import adsk.fusion

V = adsk.core.ValueInput


def run(context):
    app = adsk.core.Application.get()
    design = adsk.fusion.Design.cast(app.activeProduct)
    root = design.rootComponent
    feats = root.features
    sketch = root.sketches.add(root.xYConstructionPlane)
    sketch.sketchCurves.sketchLines.addCenterPointRectangle(
        adsk.core.Point3D.create(0, 0, 0), adsk.core.Point3D.create(3, 2, 0)
    )
    box = feats.extrudeFeatures.addSimple(
        sketch.profiles.item(0), V.createByReal(2.0), adsk.fusion.FeatureOperations.NewBodyFeatureOperation
    )
    body = box.bodies.item(0)
    body.name = "Box"
    rim = box.endFaces.item(0)
    faces = adsk.core.ObjectCollection.create()
    faces.add(rim)
    shell = feats.shellFeatures.createInput(faces, False)
    shell.insideThickness = V.createByReal(0.2)
    feats.shellFeatures.add(shell)
    web_sketch = root.sketches.add(rim)
    line = web_sketch.sketchCurves.sketchLines.addByTwoPoints(
        adsk.core.Point3D.create(0, -1.9, 0), adsk.core.Point3D.create(0, 1.9, 0)
    )
    web_in = feats.webFeatures.createInput([line], V.createByReal(0.2), body)
    web_in.draftAngle = V.createByReal(0)
    web_in.isFlipped = True
    web_in.filletRadius = V.createByString("1 mm")
    web_in.setToNextExtent()
    feats.webFeatures.add(web_in)
