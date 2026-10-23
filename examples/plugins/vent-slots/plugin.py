#!/usr/bin/env python3
"""Vent slots: an example R-CAD plugin served by an external program.

Reads one JSON request from standard input and writes one JSON answer to
standard output ({"ok": ...} or {"error": "..."}). Standard library only.
"""
import json
import math
import sys

# Height above the face the cutting tool reaches, so its top never lies in
# the face's plane (mm).
CLEARANCE = 1.0
# Extra depth below the body for a through cut (mm).
THROUGH_MARGIN = 1.0


class Refusal(Exception):
    """A request this feature does not support (answered as an error)."""


def rotate(p, c, s):
    return [p[0] * c - p[1] * s, p[0] * s + p[1] * c]


def slot_profile(offset, width, length, c, s):
    """Rounded slot of overall `length` along the (rotated) x axis and
    `width` across, centred at `offset` along the (rotated) y axis: two
    straight sides and two semicircular ends, counter-clockwise. A slot as
    long as it is wide is a circle."""
    r = width / 2.0
    a = (length - width) / 2.0  # half length of the straight sides
    if a <= 0.0:
        return {"shape": "circle", "center": rotate([0.0, offset], c, s), "radius": r}

    def pt(u, v):
        return rotate([u, offset + v], c, s)

    return {
        "shape": "loop",
        "segments": [
            {"seg": "line", "start": pt(-a, -r)},
            {"seg": "arc", "start": pt(a, -r), "center": pt(a, 0.0), "ccw": True},
            {"seg": "line", "start": pt(a, r)},
            {"seg": "arc", "start": pt(-a, r), "center": pt(-a, 0.0), "ccw": True},
        ],
    }


def build(req):
    if req.get("feature") != "vent_slots":
        raise Refusal("unknown feature '%s'" % req.get("feature"))
    p = req["params"]
    count = int(p["count"])
    width = float(p["slot_width"])
    length = float(p["slot_length"])
    pitch = float(p["pitch"])
    depth = float(p["depth"])
    angle = math.radians(float(p["rotation_deg"]))
    face = req["inputs"]["face"]
    if face.get("kind") != "face":
        raise Refusal("input 'face' must be a face")
    if length < width:
        raise Refusal("slot_length (%g) must be at least slot_width (%g)" % (length, width))
    if count > 1 and pitch <= width:
        raise Refusal("pitch (%g) must exceed slot_width (%g) so the slots stay apart" % (pitch, width))
    if depth == 0.0:
        # Through: from below the lowest point of the body.
        bottom = min(float(face["body_span"][0]), 0.0) - THROUGH_MARGIN
    else:
        bottom = -depth
    c, s = math.cos(angle), math.sin(angle)
    ops = []
    for i in range(count):
        offset = (i - (count - 1) / 2.0) * pitch
        ops.append({
            "op": "prism",
            # Stable across parameter edits: slot i keeps id i.
            "id": i,
            "body": "face",
            "frame": face["frame"],
            "profile": slot_profile(offset, width, length, c, s),
            "from": bottom,
            "to": CLEARANCE,
            "mode": "cut",
            "label": "slot %d" % (i + 1),
        })
    return ops


def answer(msg):
    method = msg.get("method")
    if method == "build":
        return build(msg["request"])
    if method == "validate_data":
        return None
    if method == "data_effects":
        return {"parameters": [], "materials": []}
    if method == "rule_symbols":
        return {}
    if method == "analyze":
        raise Refusal("this plugin has no analyses")
    raise Refusal("unknown method '%s'" % method)


def main():
    try:
        msg = json.loads(sys.stdin.buffer.read())
        out = {"ok": answer(msg)}
    except Refusal as e:
        out = {"error": str(e)}
    except (KeyError, TypeError, ValueError) as e:
        out = {"error": "malformed request: %r" % (e,)}
    sys.stdout.write(json.dumps(out))
    sys.stdout.write("\n")


if __name__ == "__main__":
    main()
