"""Minimal helper for writing R-CAD plugins in Python (standard library only).

Copy this file next to your plugin's script. It speaks the plugin protocol of
docs/PLUGINS.md: one JSON request on standard input, one JSON answer on
standard output. A plugin registers a build function per feature (and
optionally analyses and data hooks) and calls `run()`:

    from rcad_plugin import Plugin, Refusal, circle, prism

    plugin = Plugin()

    @plugin.feature("my_feature")
    def build(req):
        face = req["inputs"]["face"]
        return [prism(0, "face", face["frame"], circle([0, 0], 2), -1, 5, "cut")]

    plugin.run()

Raise `Refusal("why")` for input the feature does not support; the user
sees the message and the document is unchanged.
"""
import json
import sys


class Refusal(Exception):
    """A request the plugin declines (answered as {"error": ...})."""


# Profiles (plane coordinates of an operation's frame, mm).
def circle(center, radius):
    return {"shape": "circle", "center": list(center), "radius": float(radius)}


def polygon(points):
    return {"shape": "polygon", "points": [list(p) for p in points]}


def rect(center, width, height):
    cx, cy = center
    w, h = width / 2.0, height / 2.0
    return polygon([[cx - w, cy - h], [cx + w, cy - h], [cx + w, cy + h], [cx - w, cy + h]])


# Frames: {"origin", "x", "y", "z"} as lists of three floats.
def frame_point(frame, p, h=0.0):
    """World point at plane coordinates `p` and height `h` along the normal."""
    o, x, y, z = frame["origin"], frame["x"], frame["y"], frame["z"]
    return [o[i] + x[i] * p[0] + y[i] * p[1] + z[i] * h for i in range(3)]


def frame_at(frame, p, h=0.0):
    """The frame moved to plane point `p` and height `h` (axes unchanged)."""
    return dict(frame, origin=frame_point(frame, p, h))


def prism(op_id, body, frame, profile, from_, to, mode, holes=(), taper_deg=0.0, label=""):
    """A prism operation: `profile` minus `holes` swept along the frame
    normal over [from_, to], joined to ("join") or cut from ("cut") the body
    of input `body`. Keep `op_id` stable across parameter edits."""
    op = {"op": "prism", "id": int(op_id), "body": body, "frame": frame, "profile": profile,
          "from": float(from_), "to": float(to), "mode": mode, "label": label}
    if holes:
        op["holes"] = list(holes)
    if taper_deg:
        op["taper_deg"] = float(taper_deg)
    return op


class Plugin:
    def __init__(self):
        self._features = {}
        self._analyses = {}
        self.validate_data = lambda data: None
        self.data_effects = lambda data: {"parameters": [], "materials": []}
        self.rule_symbols = lambda data, body: {}

    def feature(self, name):
        def deco(f):
            self._features[name] = f
            return f
        return deco

    def analysis(self, name):
        def deco(f):
            self._analyses[name] = f
            return f
        return deco

    def _answer(self, msg):
        m = msg.get("method")
        if m == "build":
            req = msg["request"]
            f = self._features.get(req.get("feature"))
            if f is None:
                raise Refusal("no feature '%s'" % req.get("feature"))
            return f(req)
        if m == "analyze":
            req = msg["request"]
            a = self._analyses.get(req.get("analysis"))
            if a is None:
                raise Refusal("no analysis '%s'" % req.get("analysis"))
            return a(req)
        if m == "validate_data":
            return self.validate_data(msg.get("data"))
        if m == "data_effects":
            return self.data_effects(msg.get("data"))
        if m == "rule_symbols":
            return self.rule_symbols(msg.get("data"), msg.get("body"))
        raise Refusal("unknown method '%s'" % m)

    def run(self):
        try:
            msg = json.loads(sys.stdin.read())
            out = {"ok": self._answer(msg)}
        except Refusal as e:
            out = {"error": str(e)}
        except (KeyError, TypeError, ValueError) as e:
            out = {"error": "invalid request: %s" % e}
        sys.stdout.write(json.dumps(out))
        sys.stdout.write("\n")
