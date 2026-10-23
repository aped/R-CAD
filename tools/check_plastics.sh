#!/usr/bin/env bash
# Independent FreeCAD (OpenCascade) validation of plastics plugin geometry (docs/PLASTICS.md).
# usage: tools/heavy.sh tools/check_plastics.sh [DIR]   (default artifacts/plastics_check)
# Builds cases with the release rcad-cli (RCAD_CLI overrides the binary), exports each body to
# STEP and checks with FreeCAD: exactly one valid solid, and its volume
#   - against a closed form computed here (bosses with gussets, a drafted rest, a lip and its
#     groove, a snap-fit groove), to 5e-5 relative: OpenCascade's integration of our exports
#     differs by up to ~1e-5 relative (no pcurves on planar faces, see tools/check_revolved.sh);
#     the command tests assert the exact volumes to 1e-6;
#   - for the full plastic enclosure example (no closed form), against R-CAD's own volume to
#     5e-5 relative: a cross-check of the export, labelled as such.
set -u -o pipefail
REPO=$(cd "$(dirname "$0")/.." && pwd)
DIR=${1:-$REPO/artifacts/plastics_check}
mkdir -p "$DIR"; DIR=$(realpath "$DIR")
CLI=${RCAD_CLI:-$REPO/target/release/rcad-cli}
if ! flatpak info org.freecad.FreeCAD >/dev/null 2>&1; then
  echo "FreeCAD flatpak not installed: plastics STEP validation NOT RUN"; exit 2
fi
if [ ! -x "$CLI" ]; then
  (cd "$REPO" && cargo build --release -p rcad-cli) || exit 2
fi
python3 - "$DIR" "$REPO" <<'PY' || exit 2
import json, math, sys
d, repo = sys.argv[1], sys.argv[2]
def face(f, role): return {"feature": f, "role": role}
def offp(f, role, z): return {"Offset": {"base": {"Face": face(f, role)}, "distance": z}}
def rect(plane, w, h, c=None):
    x = {"cmd": "create_sketch", "plane": plane, "template": "centered_rectangle", "width": w, "height": h}
    if c: x["center"] = c
    return x
def pts(plane, ps):
    return {"cmd": "create_sketch", "plane": plane, "template": "custom", "sketch": {
        "points": [{"id": i, "x": x, "y": y} for i, (x, y) in enumerate(ps)], "entities": [],
        "constraints": [{"FixedPoint": {"p": i, "x": x, "y": y}} for i, (x, y) in enumerate(ps)]}}
def pf(feat, inputs, params=None, choices=None):
    return {"cmd": "plugin_feature", "plugin": "plastics", "feature": feat, "inputs": inputs,
            "params": params or {}, "choices": choices or {}}
def sk(i): return {"kind": "sketch", "sketch": i}
def fc(p): return {"kind": "face", "plane": p}
def step(name, bodies): return [{"cmd": "export_step", "path": f"{d}/{name}_b{b}.step", "bodies": [b]} for b in bodies]
open_box = [{"cmd": "new_document"}, rect("WorldXY", 60.0, 40.0), {"cmd": "extrude", "sketch": 1, "distance": 20.0},
            {"cmd": "shell_face", "face": face(2, "ExtrudeEndCap"), "thickness": 2.0}]
BOX = 60 * 40 * 20 - 56 * 36 * 18
cases = {}
# A. Plate 60x40x4 with two bosses (h 8, d 6, pilot 2.5 x 6) and four gussets each.
def rib_outside(r, t, l, ch, h0, h1, n=20000):
    # Flat top h0 to l - ch, then a straight chamfer down to h1 at l.
    xs = l - ch
    f = lambda y: h0 * (xs - math.sqrt(r * r - y * y)) + ch * (h0 + h1) / 2
    dy = t / n
    s = f(-t / 2) + f(t / 2) + sum((4 if i % 2 else 2) * f(-t / 2 + i * dy) for i in range(1, n))
    return s * dy / 3
cases["boss_ribs"] = ([{"cmd": "new_document"}, rect("WorldXY", 60.0, 40.0), {"cmd": "extrude", "sketch": 1, "distance": 4.0},
    pts({"Face": face(2, "ExtrudeEndCap")}, [(-15.0, 0.0), (15.0, 0.0)]),
    pf("boss", {"sketch": sk(3)}, {"height": 8.0, "draft_deg": 0.0, "rib_count": 4, "rib_thickness": 1.2,
       "rib_length": 7.0, "rib_height": 6.0, "rib_tip_height": 2.0, "rib_angle_deg": 45.0})],
    {1: 9600 + 2 * (math.pi * 9 * 8 + 4 * rib_outside(3, 1.2, 7, 3, 6, 2) - math.pi * 1.25 ** 2 * 6)})
# B. Drafted rest (2 deg) in an open box.
t = math.tan(math.radians(2))
sq = lambda c, k, a, b: (-(c - k * b) ** 3 + (c - k * a) ** 3) / (3 * k)
cases["rest_drafted"] = (open_box + [rect(offp(2, "ExtrudeEndCap", -8.0), 10.0, 10.0),
    pf("rest", {"sketch": sk(4)}, {"thickness": 2.0, "draft_deg": 2.0})],
    {1: BOX + sq(10 + 24 * t, 2 * t, 2, 12) - sq(6 + 24 * t, 2 * t, 0, 10)})
# C. Lip and groove between the box and a lid shelled open at its bottom.
cases["lip_groove"] = (open_box + [rect({"Offset": {"base": "WorldXY", "distance": 20.0}}, 60.0, 40.0),
    {"cmd": "extrude", "sketch": 4, "distance": 10.0},
    {"cmd": "shell_face", "face": face(5, "ExtrudeStartCap"), "thickness": 2.0},
    pf("lip", {"face": fc({"Face": face(2, "ExtrudeEndCap")}), "mate": fc({"Face": face(5, "ExtrudeStartCap")})},
       {"width": 1.0, "height": 2.0, "clearance": 0.1}, {"type": "lip_and_groove"})],
    {1: BOX + (58 * 38 - 56 * 36) * 2, 2: 24000 - 56 * 36 * 8 - (58.2 * 38.2 - 56 * 36) * 2.1})
# D. Snap hook and the window it cuts in a block beside it.
v2 = 8 - 0.7 / math.tan(math.radians(30))
hook = 1 * 8 + 0.7 * (v2 - 6) + 0.35 * (8 - v2)
cases["snap_groove"] = ([{"cmd": "new_document"}, rect("WorldXY", 60.0, 40.0), {"cmd": "extrude", "sketch": 1, "distance": 4.0},
    pts({"Face": face(2, "ExtrudeEndCap")}, [(0.0, 0.0)]),
    rect({"Offset": {"base": "WorldXY", "distance": 6.0}}, 3.5, 20.0, [3.25, 0.0]),
    {"cmd": "extrude", "sketch": 4, "distance": 8.0},
    pf("snap_fit", {"sketch": sk(3), "mate": {"kind": "body", "body": 2}},
       {"thickness": 1.0, "tip_thickness": 1.0, "catch_depth": 0.7, "catch_height": 2.0, "length": 8.0, "width": 5.0},
       {"type": "hook_and_groove"})],
    {1: 9600 + 5 * hook, 2: 3.5 * 20 * 8 - 0.3 * 2.2 * 5.2})
# F. Bosses with tip fillets (r 0.5): removed A*P - M*2pi per boss (docs/FILLETS.md).
r = 0.5
A, M = r * r * (1 - math.pi / 4), r ** 3 * (5 / 6 - math.pi / 4)
cases["boss_tip"] = ([{"cmd": "new_document"}, rect("WorldXY", 60.0, 40.0), {"cmd": "extrude", "sketch": 1, "distance": 4.0},
    pts({"Face": face(2, "ExtrudeEndCap")}, [(-15.0, 0.0), (15.0, 0.0)]),
    pf("boss", {"sketch": sk(3)}, {"height": 8.0, "draft_deg": 0.0, "tip_radius": r})],
    {1: 9600 + 2 * (math.pi * 9 * 8 - math.pi * 1.25 ** 2 * 6 - (A * 2 * math.pi * 3 - M * 2 * math.pi))})
# G. Drafted box (3 deg walls, fixed bottom): prismatoid.
tt = math.tan(math.radians(3))
area = lambda z: (40 - 2 * tt * z) * (30 - 2 * tt * z)
cases["draft_box"] = ([{"cmd": "new_document"}, rect("WorldXY", 40.0, 30.0), {"cmd": "extrude", "sketch": 1, "distance": 20.0},
    pf("draft", {"faces": {"kind": "faces", "faces": [face(2, {"ExtrudeSide": {"entity": e}}) for e in range(1, 5)]},
                 "fixed": fc({"Face": face(2, "ExtrudeStartCap")})}, {"draft_deg": 3.0})],
    {1: 20 / 6 * (area(0) + 4 * area(10) + area(20))})
# H. Half-circle web (r 10, 2 thick) from the rim to the floor of an open box.
arc_sketch = {"cmd": "create_sketch", "plane": {"Face": face(2, "ExtrudeEndCap")}, "template": "custom", "sketch": {
    "points": [{"id": 0, "x": 0.0, "y": 0.0}, {"id": 1, "x": 10.0, "y": 0.0}, {"id": 2, "x": -10.0, "y": 0.0}],
    "entities": [{"Arc": {"id": 1, "center": 0, "start": 1, "end": 2}}],
    "constraints": [{"FixedPoint": {"p": i, "x": x, "y": 0.0}} for i, x in [(0, 0.0), (1, 10.0), (2, -10.0)]]}}
cases["arc_web"] = (open_box + [arc_sketch,
    pf("web", {"sketch": sk(4)}, {"thickness": 2.0, "draft_deg": 0.0}, {"direction": "flip", "extent": "to_next"})],
    {1: BOX + math.pi / 2 * (11 ** 2 - 9 ** 2) * 18})
# I. Bosses with root fillets (r 1): Pappus on the spandrel f^2 (1 - pi/4).
f = 1.0
ring = 2 * math.pi * (3 + f * (10 - 3 * math.pi) / (12 - 3 * math.pi)) * f * f * (1 - math.pi / 4)
cases["boss_root"] = ([{"cmd": "new_document"}, rect("WorldXY", 60.0, 40.0), {"cmd": "extrude", "sketch": 1, "distance": 4.0},
    pts({"Face": face(2, "ExtrudeEndCap")}, [(-15.0, 0.0), (15.0, 0.0)]),
    pf("boss", {"sketch": sk(3)}, {"height": 8.0, "draft_deg": 0.0, "root_radius": f})],
    {1: 9600 + 2 * (math.pi * 9 * 8 - math.pi * 1.25 ** 2 * 6 + ring)})
# J. Drafted lip and groove (5 deg): band 184u + 4u^2 integrated over the height.
t5 = math.tan(math.radians(5))
band = lambda c, l: 184 * (c * l - t5 * l * l / 2) + 4 * (c ** 3 - (c - t5 * l) ** 3) / (3 * t5)
cases["lip_drafted"] = (open_box + [rect({"Offset": {"base": "WorldXY", "distance": 20.0}}, 60.0, 40.0),
    {"cmd": "extrude", "sketch": 4, "distance": 10.0},
    {"cmd": "shell_face", "face": face(5, "ExtrudeStartCap"), "thickness": 2.0},
    pf("lip", {"face": fc({"Face": face(2, "ExtrudeEndCap")}), "mate": fc({"Face": face(5, "ExtrudeStartCap")})},
       {"width": 1.0, "height": 2.0, "clearance": 0.1, "draft_deg": 5.0}, {"type": "lip_and_groove"})],
    {1: BOX + band(1.0, 2.0), 2: 24000 - 56 * 36 * 8 - band(1.1, 2.1)})
# K. Two-sided draft (3 deg) about the mid-plane of a 40 x 30 x 20 box.
area3 = lambda z: (40 - 2 * tt * z) * (30 - 2 * tt * z)
cases["draft_two_sided"] = ([{"cmd": "new_document"}, rect("WorldXY", 40.0, 30.0), {"cmd": "extrude", "sketch": 1, "distance": 20.0},
    pf("draft", {"faces": {"kind": "faces", "faces": [face(2, {"ExtrudeSide": {"entity": e}}) for e in range(1, 5)]},
                 "fixed": fc({"Offset": {"base": {"Face": face(2, "ExtrudeStartCap")}, "distance": -10.0}})},
       {"draft_deg": 3.0}, {"sides": "two_sides"})],
    {1: 2 * 10 / 6 * (area3(0) + 4 * area3(5) + area3(10))})
# E. The plastic enclosure example (no closed form: cross-check against R-CAD's volumes).
ex = json.load(open(f"{repo}/examples/plastic_enclosure_workflow.json"))
ex = [c for c in ex if c["cmd"] != "save"]
cases["enclosure"] = (ex + [{"cmd": "mass_properties", "bodies": [1, 2]}], {1: None, 2: None})
for name, (cmds, exp) in cases.items():
    json.dump(cmds + step(name, exp.keys()) + [{"cmd": "save", "path": f"{d}/{name}.rcad.json"}], open(f"{d}/{name}.json", "w"))
    json.dump({str(k): v for k, v in exp.items()}, open(f"{d}/{name}.expect.json", "w"))
PY
status=0
for script in "$DIR"/*.json; do
  case "$script" in *.expect.json|*.rcad.json) continue;; esac
  name=$(basename "$script" .json)
  "$CLI" run "$script" > "$DIR/$name.log" 2>&1 || { echo "$name: rcad-cli FAILED (see $DIR/$name.log)"; status=1; continue; }
  for stepf in "$DIR/${name}"_b*.step; do
    b=${stepf##*_b}; b=${b%.step}
    out=$(timeout 300 flatpak run --env="RCAD_STEP=$stepf" --filesystem="$DIR" --filesystem="$REPO" \
      --command=FreeCADCmd org.freecad.FreeCAD "$REPO/tools/freecad_check_step.py" 2>&1 | grep FREECAD_RESULT) \
      || { echo "$name body $b: FreeCAD failed"; status=1; continue; }
    echo "$out" > "${stepf%.step}_freecad.txt"
    python3 - "$name" "$b" "$out" "$DIR/$name.expect.json" "$DIR/$name.log" <<'PY' || status=1
import json, sys
name, b, out, expf, log = sys.argv[1:6]
r = json.loads(out.split(" ", 1)[1])
exp = json.load(open(expf)).get(b)
label = "analytic"
if exp is None:
    for line in open(log):
        rr = json.loads(line).get("response", {})
        for x in (rr.get("mass_properties") or {}).get("bodies", []):
            if x["body"] == int(b) and x.get("properties"):
                exp = x["properties"]["volume"]
    label = "R-CAD volume (cross-check)"
err = abs(r.get("volume", 0) - exp) / max(abs(exp), 1.0) if exp is not None else float("inf")
ok = bool(r.get("ok") and r.get("isValid") and r.get("solids") == 1 and err <= 5e-5)
print(f"{name} body {b}: solids={r.get('solids')} valid={r.get('isValid')} volume={r.get('volume', 0):.6f} "
      f"expected={exp} ({label}) rel_err={err:.2e} -> {'PASS' if ok else 'FAIL'}")
sys.exit(0 if ok else 1)
PY
  done
done
[ $status = 0 ] && echo "plastics STEP validation: PASS" || echo "plastics STEP validation: FAIL"
exit $status
