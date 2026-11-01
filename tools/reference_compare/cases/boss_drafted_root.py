"""Two bosses drafted 2 degrees with 1 mm root fillets."""

import os
import sys

import adsk.core

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import _plate  # noqa: E402

V = adsk.core.ValueInput


def run(context):
    root, points = _plate.plate_with_points()
    _plate.boss(root, points, draftAngle=V.createByString("2 deg"), rootRadius=V.createByString("1 mm"))
