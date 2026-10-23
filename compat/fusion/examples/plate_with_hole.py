"""A 100 x 60 mm plate, `thick` thick, with a 15 mm hole cut through it.

Run with:  rcad-cli fusion-script compat/fusion/examples/plate_with_hole.py --out plate.rcad.json

Lengths given as reals are in centimetres (the API's internal unit); the
plate thickness is a user parameter, and both extrusions are bound to it.
"""

import traceback

import adsk.cam
import adsk.core
import adsk.fusion


def run(context):
    ui = None
    try:
        app = adsk.core.Application.get()
        ui = app.userInterface
        design = adsk.fusion.Design.cast(app.activeProduct)
        root = design.rootComponent

        # 0.8 cm = 8 mm.
        design.userParameters.add(
            "thick", adsk.core.ValueInput.createByReal(0.8), "mm", "plate thickness"
        )

        # Plate: centred rectangle with a corner at (5, 3) cm.
        sketch = root.sketches.add(root.xYConstructionPlane)
        sketch.sketchCurves.sketchLines.addCenterPointRectangle(
            adsk.core.Point3D.create(0, 0, 0), adsk.core.Point3D.create(5, 3, 0)
        )
        plate = root.features.extrudeFeatures.addSimple(
            sketch.profiles.item(0),
            adsk.core.ValueInput.createByString("thick"),
            adsk.fusion.FeatureOperations.NewBodyFeatureOperation,
        )
        plate.bodies.item(0).name = "Plate"

        # Hole: a 7.5 mm radius circle on the top face at (20, 10) mm, cut
        # down through the plate.
        top = plate.endFaces.item(0)
        hole_sketch = root.sketches.add(top)
        hole_sketch.sketchCurves.sketchCircles.addByCenterRadius(
            adsk.core.Point3D.create(2, 1, 0), 0.75
        )
        cut = root.features.extrudeFeatures.createInput(
            hole_sketch.profiles.item(0), adsk.fusion.FeatureOperations.CutFeatureOperation
        )
        cut.setDistanceExtent(False, adsk.core.ValueInput.createByString("-thick"))
        root.features.extrudeFeatures.add(cut)
        ui.messageBox("Plate with hole built", "plate_with_hole")
    except:  # noqa: E722 - the usual Fusion script pattern
        if ui:
            ui.messageBox("Failed:\n{}".format(traceback.format_exc()))
