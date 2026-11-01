"""Two plain bosses on a plate.
Closed form: 60*40*4 + 2*(pi*3^2*8 - pi*1.25^2*6) mm^3."""

import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import _plate  # noqa: E402


def run(context):
    root, points = _plate.plate_with_points()
    _plate.boss(root, points)
