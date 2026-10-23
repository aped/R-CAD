#!/usr/bin/env python3
"""Standoffs: PCB mounting posts at points on a face, each with a pilot hole.
An example R-CAD plugin written with the rcad_plugin.py helper."""
import os
import sys

sys.path.insert(0, os.environ.get("RCAD_PLUGIN_DIR", os.path.dirname(os.path.abspath(__file__))))
from rcad_plugin import Plugin, Refusal, circle, frame_at, prism  # noqa: E402

plugin = Plugin()


@plugin.feature("standoffs")
def build(req):
    p = req["params"]
    face = req["inputs"]["face"]
    points = req["inputs"]["points"]["points"]
    if not points:
        raise Refusal("give at least one point")
    od, hd, h, depth = p["diameter"], p["hole_diameter"], p["height"], p["hole_depth"]
    if hd >= od:
        raise Refusal("the hole must be narrower than the post")
    if depth > h:
        raise Refusal("the hole must not be deeper than the post")
    ops = []
    for i, pt in enumerate(points):
        at = frame_at(face["frame"], pt)
        ops.append(prism(2 * i, "face", at, circle([0, 0], od / 2), 0, h, "join", label="post %d" % (i + 1)))
        ops.append(prism(2 * i + 1, "face", at, circle([0, 0], hd / 2), h - depth, h + 1, "cut",
                         label="post %d hole" % (i + 1)))
    return ops


plugin.run()
