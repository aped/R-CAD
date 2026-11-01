"""Two bosses with 0.5 mm tip fillets."""

import os
import sys

import adsk.core

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import _plate  # noqa: E402


def run(context):
    root, points = _plate.plate_with_points()
    _plate.boss(root, points, tipRadius=adsk.core.ValueInput.createByString("0.5 mm"))
