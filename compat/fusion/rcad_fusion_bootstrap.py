"""Runs a script written for a subset of the Fusion add-in API surface and
writes the recorded R-CAD commands as JSON.

    python3 -I rcad_fusion_bootstrap.py SCRIPT.py OUT.json
    python3 -I rcad_fusion_bootstrap.py --addin REQUEST.json OUT.json

Used by ``rcad-cli fusion-script``. The script is imported as a module and
its ``run(context)`` is called (then ``stop(context)`` if it has one). OUT
receives ``{"steps": [...]}`` or ``{"error": {"kind", "message",
"traceback"}}``; the exit status is 0 or 1 accordingly.

With ``--addin`` (``rcad-cli fusion-addin``), REQUEST is ``{"addin": DIR,
"script": DIR/NAME.py, "list": bool, "command": ID or null, "inputs": [[ID,
TEXT], ...], "base": SCRIPT or null}`` and the add-in is run by
``adsk._addin``; OUT also gets ``"addin"`` (the command run) or ``"listing"``
(the registered commands, with ``list``).
"""

import importlib.util
import json
import os
import sys
import traceback

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))


def _error(kind, message, tb=""):
    return {"error": {"kind": kind, "message": message, "traceback": tb}}


def execute(script):
    import adsk
    import adsk.cam  # noqa: F401
    import adsk.core  # noqa: F401
    import adsk.fusion  # noqa: F401

    rec = adsk.reset()
    try:
        path = os.path.abspath(script)
        # Like Fusion, let the script import modules next to it.
        sys.path.append(os.path.dirname(path))
        stem = os.path.splitext(os.path.basename(path))[0]
        name = stem if stem.isidentifier() else "fusion_script"
        spec = importlib.util.spec_from_file_location(name, path)
        if spec is None or spec.loader is None:
            return _error("python", "cannot load %s as a Python module" % script)
        mod = importlib.util.module_from_spec(spec)
        sys.modules[name] = mod
        spec.loader.exec_module(mod)
        run = getattr(mod, "run", None)
        if not callable(run):
            return _error("python", "the script has no run(context) function")
        context = {"IsApplicationStartup": False}
        run(context)
        stop = getattr(mod, "stop", None)
        if callable(stop):
            stop(context)
        # A NotSupported the script caught (and perhaps showed in a message
        # box) still fails the run: the model would be incomplete.
        if rec.not_supported:
            return _error(
                "not_supported",
                rec.not_supported[0] + " (raised inside the script, which caught it)",
            )
        if rec.script_error is not None:
            return _error("script", "the script reported an error:\n" + rec.script_error)
        return rec.finish()
    except adsk.NotSupported as e:
        return _error("not_supported", str(e), traceback.format_exc())
    except BaseException as e:  # noqa: BLE001 - report everything, SystemExit included
        return _error("python", "%s: %s" % (type(e).__name__, e), traceback.format_exc())


def execute_addin(request_path):
    try:
        with open(request_path, encoding="utf-8") as f:
            request = json.load(f)
    except (OSError, ValueError) as e:
        return _error("python", "cannot read the add-in request: %s" % e)
    from adsk import _addin

    return _addin.execute(request)


def main(argv):
    if len(argv) == 4 and argv[1] == "--addin":
        result = execute_addin(argv[2])
        out = argv[3]
    elif len(argv) == 3:
        result = execute(argv[1])
        out = argv[2]
    else:
        sys.stderr.write("usage: rcad_fusion_bootstrap.py [--addin REQUEST.json | SCRIPT.py] OUT.json\n")
        return 2
    sys.stdout.flush()
    with open(out, "w", encoding="utf-8") as f:
        json.dump(result, f)
    return 1 if "error" in result else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
