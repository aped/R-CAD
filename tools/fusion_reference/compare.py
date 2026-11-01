#!/usr/bin/env python3
"""Compare R-CAD with Autodesk Fusion on the reference cases (docs/FUSION_SCRIPTS.md).

usage: tools/fusion_reference/compare.py [--fusion RESULTS.json] [--cli RCAD_CLI] [--out DIR]

Each case is a Fusion API script. R-CAD runs it with `rcad-cli fusion-script` and measures
the result; Fusion's measurements come from running RcadReference in Fusion, which writes
fusion_results.json. Per body (matched by name, else by order):
  - volume, to 1e-5 relative (both are exact B-rep integrals; Fusion at its very high
    accuracy setting);
  - centre of mass, to 1e-3 mm;
  - bounding box, to 1e-3 mm, advisory only (Fusion's body bounding box may be loose).
Cases with a hand-derived closed form also check R-CAD's volume against it (1e-9 relative),
so a wrong case script is caught before blaming either modeller.

Exit status: 0 every compared value agrees; 1 a mismatch, an error, or a case missing from
the Fusion results; 2 no Fusion results (R-CAD's side is still measured and written).
"""
import argparse
import json
import math
import os
import subprocess
import sys

REPO = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
HERE = os.path.join(REPO, "tools", "fusion_reference")
CASES = [
    "tools/fusion_reference/cases/boss_plain.py",
    "tools/fusion_reference/cases/boss_tip.py",
    "tools/fusion_reference/cases/boss_drafted_root.py",
    "tools/fusion_reference/cases/web_fillet_box.py",
    "compat/fusion/examples/plate_with_hole.py",
    "compat/fusion/examples/plastic_box.py",
]
PI = math.pi
CLOSED = {
    "boss_plain": 60 * 40 * 4 + 2 * (PI * 9 * 8 - PI * 1.25**2 * 6),
    "web_fillet_box": 60 * 40 * 20 - 56 * 36 * 18 + 36 * 2 * 18 + 2 * 36 * (1 - PI / 4),
    "plate_with_hole": 100 * 60 * 8 - PI * 7.5**2 * 8,
    "plastic_box": 80 * 50 * 25 - 76 * 46 * 23 + 4 * (PI * 9 * 15 - PI * 1.25**2 * 6)
    + 46 * 2 * 23 + 76 * 2 * 13 - 2 * 2 * 13,
}
VOL_REL, COM_ABS, BOX_ABS = 1e-5, 1e-3, 1e-3


def rcad_side(cli, case, out):
    name = os.path.splitext(os.path.basename(case))[0]
    doc = os.path.join(out, name + ".rcad.json")
    r = subprocess.run([cli, "fusion-script", os.path.join(REPO, case), "--out", doc],
                       capture_output=True, text=True)
    if r.returncode != 0:
        return name, {"error": (r.stderr or r.stdout).strip()[-2000:]}
    script = os.path.join(out, name + ".measure.json")
    json.dump([{"cmd": "open", "path": doc}, {"cmd": "inspect"}, {"cmd": "mass_properties"}],
              open(script, "w"))
    r = subprocess.run([cli, "run", script], capture_output=True, text=True)
    if r.returncode != 0:
        return name, {"error": (r.stderr or r.stdout).strip()[-2000:]}
    summary, mass = None, None
    for line in r.stdout.splitlines():
        resp = json.loads(line).get("response", {})
        summary = resp.get("summary") or summary
        mass = resp.get("mass_properties") or mass
    props = {b["body"]: b.get("properties") or {} for b in mass["bodies"]}
    bodies = []
    for b in summary["bodies"]:
        p = props.get(b["id"], {})
        bodies.append({"name": b["name"], "volume_mm3": p.get("volume"),
                       "centroid_mm": p.get("centroid"),
                       "bbox_mm": [b["bounds"]["min"], b["bounds"]["max"]]})
    return name, {"bodies": bodies}


def fusion_bodies(entry):
    return [{"name": b["name"], "volume_mm3": b["volume_cm3"] * 1000.0,
             "centroid_mm": [c * 10.0 for c in b["centroid_cm"]],
             "bbox_mm": [[c * 10.0 for c in b["bbox_cm"][0]], [c * 10.0 for c in b["bbox_cm"][1]]],
             "is_solid": b.get("is_solid", True)}
            for b in entry["bodies"]]


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--fusion", default=os.path.join(HERE, "RcadReference", "fusion_results.json"))
    ap.add_argument("--cli", default=os.environ.get("RCAD_CLI", os.path.join(REPO, "target", "release", "rcad-cli")))
    ap.add_argument("--out", default=os.path.join(REPO, "artifacts", "fusion_reference"))
    a = ap.parse_args()
    os.makedirs(a.out, exist_ok=True)
    out = os.path.realpath(a.out)
    fusion = json.load(open(a.fusion)) if os.path.exists(a.fusion) else None
    ok, report = True, {"fusion_version": fusion and fusion.get("fusion_version"), "cases": {}}
    for case in CASES:
        name, ours = rcad_side(a.cli, case, out)
        rep = {"rcad": ours}
        report["cases"][name] = rep
        if "error" in ours:
            print(f"{name}: R-CAD FAILED: {ours['error'].splitlines()[-1]}")
            ok = False
            continue
        vol = sum(b["volume_mm3"] or 0.0 for b in ours["bodies"])
        if name in CLOSED:
            err = abs(vol - CLOSED[name]) / CLOSED[name]
            good = err <= 1e-9
            ok &= good
            print(f"{name}: R-CAD volume {vol:.6f} vs closed form {CLOSED[name]:.6f} "
                  f"rel {err:.1e} -> {'PASS' if good else 'FAIL'}")
        if fusion is None:
            continue
        entry = fusion["cases"].get(name)
        if entry is None or "error" in entry:
            why = "missing" if entry is None else entry["error"].strip().splitlines()[-1]
            print(f"{name}: Fusion result {why} -> FAIL")
            ok = False
            continue
        theirs = fusion_bodies(entry)
        rep["fusion"] = theirs
        by_name = {b["name"]: b for b in theirs}
        if len(theirs) != len(ours["bodies"]):
            print(f"{name}: {len(ours['bodies'])} R-CAD bodies, {len(theirs)} Fusion bodies -> FAIL")
            ok = False
        for i, b in enumerate(ours["bodies"]):
            f = by_name.get(b["name"]) or (theirs[i] if i < len(theirs) else None)
            if f is None:
                print(f"{name} {b['name']}: no Fusion body -> FAIL")
                ok = False
                continue
            dv = abs(b["volume_mm3"] - f["volume_mm3"]) / max(abs(f["volume_mm3"]), 1.0)
            dc = max(abs(x - y) for x, y in zip(b["centroid_mm"], f["centroid_mm"]))
            db = max(abs(x - y) for p, q in zip(b["bbox_mm"], f["bbox_mm"]) for x, y in zip(p, q))
            good = dv <= VOL_REL and dc <= COM_ABS and f["is_solid"]
            ok &= good
            print(f"{name} {b['name']}: volume {b['volume_mm3']:.6f} vs Fusion {f['volume_mm3']:.6f} "
                  f"(rel {dv:.1e}), centroid diff {dc:.2e} mm, bbox diff {db:.2e} mm"
                  f"{' (advisory)' if db > BOX_ABS else ''} -> {'PASS' if good else 'FAIL'}")
    json.dump(report, open(os.path.join(out, "compare.json"), "w"), indent=1)
    if fusion is None:
        print(f"Fusion results not found at {a.fusion}: Fusion comparison NOT RUN "
              "(run RcadReference in Fusion first)")
        sys.exit(2 if ok else 1)
    print("Fusion comparison: " + ("PASS" if ok else "FAIL"))
    sys.exit(0 if ok else 1)


if __name__ == "__main__":
    main()
