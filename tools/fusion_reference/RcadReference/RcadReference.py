"""Run in Autodesk Fusion (Utilities > Scripts and Add-Ins > + > this folder).

Builds every reference case in a new design document and writes what Fusion
measures for each body (volume, centre of mass, bounding box, face count) to
fusion_results.json next to this script. Compare with R-CAD's results using
tools/fusion_reference/compare.py (docs/FUSION_SCRIPTS.md).

The cases are the same Fusion API scripts R-CAD runs with
`rcad-cli fusion-script`; this runner only adds the measurement.
"""

import importlib.util
import json
import os
import traceback

import adsk.core
import adsk.fusion

HERE = os.path.dirname(os.path.abspath(__file__))
# Relative to this folder; kept in step with CASES in compare.py.
CASES = [
    "../cases/boss_plain.py",
    "../cases/boss_tip.py",
    "../cases/boss_drafted_root.py",
    "../cases/web_fillet_box.py",
    "../../../compat/fusion/examples/plate_with_hole.py",
    "../../../compat/fusion/examples/plastic_box.py",
]


def _load(path):
    name = "rcad_case_" + os.path.splitext(os.path.basename(path))[0]
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def _measure(design):
    bodies = []
    for i in range(design.rootComponent.bRepBodies.count):
        body = design.rootComponent.bRepBodies.item(i)
        try:
            props = body.getPhysicalProperties(adsk.fusion.CalculationAccuracy.VeryHighCalculationAccuracy)
        except Exception:
            props = body.physicalProperties
        com = props.centerOfMass
        box = body.boundingBox
        bodies.append(
            {
                "name": body.name,
                "is_solid": body.isSolid,
                "faces": body.faces.count,
                # Fusion's internal units: cm, cm^3.
                "volume_cm3": props.volume,
                "area_cm2": props.area,
                "centroid_cm": [com.x, com.y, com.z],
                "bbox_cm": [[box.minPoint.x, box.minPoint.y, box.minPoint.z],
                            [box.maxPoint.x, box.maxPoint.y, box.maxPoint.z]],
            }
        )
    return bodies


def run(context):
    app = adsk.core.Application.get()
    results = {"fusion_version": app.version, "cases": {}}
    for rel in CASES:
        path = os.path.normpath(os.path.join(HERE, rel))
        name = os.path.splitext(os.path.basename(path))[0]
        entry = {}
        try:
            doc = app.documents.add(adsk.core.DocumentTypes.FusionDesignDocumentType)
            design = adsk.fusion.Design.cast(app.activeProduct)
            design.designType = adsk.fusion.DesignTypes.ParametricDesignType
            _load(path).run({})
            adsk.doEvents()
            entry["bodies"] = _measure(design)
            doc.close(False)
        except Exception:
            entry["error"] = traceback.format_exc()
        results["cases"][name] = entry
    out = os.path.join(HERE, "fusion_results.json")
    with open(out, "w") as f:
        json.dump(results, f, indent=1)
    app.userInterface.messageBox("Wrote " + out)
