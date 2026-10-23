"""Runs an add-in headless: ``rcad-cli fusion-addin`` (docs/FUSION_SCRIPTS.md).

The add-in's ``run(context)`` registers command definitions and connects
their ``commandCreated`` handlers. With no user interface, this module plays
the user's part for one command:

1. ``commandCreated`` with a new :class:`adsk.core.Command`; the handler adds
   the inputs and connects the command's events;
2. each value given on the command line is set on its input, firing
   ``inputChanged`` as when the user edits it;
3. ``validateInputs``; ``areInputsValid = False`` refuses the run, as the OK
   button would stay disabled; selection limits are checked;
4. ``executePreview`` in a forked process whose results are discarded,
   unless the handler sets ``isValidResult``, in which case the preview is
   run again here and kept, and ``execute`` is not fired;
5. ``execute``, then ``destroy``, then the add-in's ``stop(context)``.

The recorded R-CAD commands are returned as for a script.
"""

import importlib.util
import json
import os
import sys
import traceback
import types

import adsk
from adsk import cam, core, fusion  # noqa: F401 - loaded before adsk.reset()


class AddinError(Exception):
    """The add-in cannot be run as asked (inputs, command choice, validation)."""


def _error(kind, message, tb=""):
    return {"error": {"kind": kind, "message": message, "traceback": tb}}


def load_module(path, package=None):
    """Import ``path``. With ``package`` (a directory), the file is a module
    of a package rooted there, so the add-in's relative imports
    (``from . import commands``) work."""
    path = os.path.abspath(path)
    folder = os.path.dirname(path)
    if folder not in sys.path:
        sys.path.append(folder)
    stem = os.path.splitext(os.path.basename(path))[0]
    name = stem if stem.isidentifier() else "fusion_script"
    if package is not None:
        base = os.path.basename(os.path.abspath(package))
        pkg_name = "rcad_addin_" + "".join(c if c.isalnum() else "_" for c in base)
        pkg = types.ModuleType(pkg_name)
        pkg.__path__ = [os.path.abspath(package)]
        pkg.__package__ = pkg_name
        sys.modules[pkg_name] = pkg
        name = "%s.%s" % (pkg_name, name)
    spec = importlib.util.spec_from_file_location(name, path)
    if spec is None or spec.loader is None:
        raise AddinError("cannot load %s as a Python module" % path)
    mod = importlib.util.module_from_spec(spec)
    sys.modules[name] = mod
    spec.loader.exec_module(mod)
    return mod


# ---------------------------------------------------------------------------
# Selections given on the command line

_FACE_ROLES = ("endFaces", "startFaces", "sideFaces", "faces")


def _pick(items, which, what):
    if which == "last":
        if not items:
            raise AddinError("%s: nothing has been created yet" % what)
        return items[-1]
    try:
        n = int(which)
    except ValueError:
        raise AddinError("%s: %r is not a 1-based number or 'last'" % (what, which)) from None
    if not 1 <= n <= len(items):
        raise AddinError("%s: there are %d, not %d" % (what, len(items), n))
    return items[n - 1]


def selection_entity(spec):
    """``face:<n|last>:<endFaces|startFaces|sideFaces|faces>[:<i>]`` (the
    n-th feature created through the API so far, counting from 1, and the
    item index of the face collection, from 0) or ``body:<n|last>``."""
    rec = adsk.recorder()
    parts = spec.split(":")
    if parts[0] == "body" and len(parts) == 2:
        return _pick(rec.bodies, parts[1], "body selection")
    if parts[0] == "face" and len(parts) in (3, 4):
        feature = _pick(rec.api_features, parts[1], "face selection (features)")
        role = parts[2]
        if role not in _FACE_ROLES:
            raise AddinError("face selection: %r is not one of %s" % (role, ", ".join(_FACE_ROLES)))
        faces = getattr(type(feature), role, None)
        if faces is None:
            raise AddinError(
                "face selection: feature %s (%s) has no %s"
                % (parts[1], type(feature).__name__, role)
            )
        coll = getattr(feature, role)
        try:
            i = int(parts[3]) if len(parts) == 4 else 0
        except ValueError:
            raise AddinError("face selection: %r is not an index" % parts[3]) from None
        face = coll.item(i)
        if face is None:
            raise AddinError("face selection: %s has %d faces, no item %d" % (role, coll.count, i))
        return face
    raise AddinError(
        "%r is not a selection (face:<feature n|last>:<endFaces|startFaces|sideFaces|faces>[:<i>] "
        "or body:<n|last>)" % spec
    )


# ---------------------------------------------------------------------------
# The command flow


def _all_inputs(inputs):
    for i in inputs._items:
        yield i
        if isinstance(i, core.GroupCommandInput):
            yield from _all_inputs(i._children)


def describe(ui, definition, command=None):
    d = {
        "id": definition._id,
        "name": definition._name,
        "tooltip": definition._tooltip,
        "resourceFolder": definition._resources,
        "placements": core._placements(ui, definition),
    }
    if command is not None:
        d["inputs"] = [i._describe() for i in command._inputs._items]
    return d


def _created(definition):
    if not definition._created._handlers:
        raise AddinError("command %r has no commandCreated handler" % definition._id)
    cmd = core.Command(definition)
    definition._created._fire(core.CommandCreatedEventArgs(definition._created, cmd))
    return cmd


def _choose(ui, wanted):
    defs = ui._definitions
    ids = ", ".join(repr(d._id) for d in defs._items) or "none"
    if wanted is not None:
        d = defs.itemById(wanted)
        if d is None:
            raise AddinError("the add-in has no command %r (commands: %s)" % (wanted, ids))
        return d
    requested = [d for d in defs._requested if d.isValid]
    if requested:
        return requested[-1]
    if len(defs._items) == 1:
        return defs._items[0]
    if not defs._items:
        raise AddinError("the add-in registered no command definitions in run()")
    raise AddinError("the add-in has several commands; choose one with --command (commands: %s)" % ids)


def _set_inputs(cmd, values):
    seen = set()
    for cid, text in values:
        inp = cmd._inputs.itemById(cid)
        if inp is None:
            ids = ", ".join(repr(i._id) for i in _all_inputs(cmd._inputs)) or "none"
            raise AddinError("command %r has no input %r (inputs: %s)" % (cmd._definition._id, cid, ids))
        if not inp._visible or not inp._enabled:
            raise AddinError("input %r is %s" % (cid, "hidden" if not inp._visible else "disabled"))
        try:
            if isinstance(inp, core.SelectionCommandInput):
                if cid not in seen:
                    inp.clearSelection()
                entity = selection_entity(text)
                if not inp._accepts(entity):
                    raise AddinError(
                        "input %r: %s does not pass the selection filters %s"
                        % (cid, text, inp._filters)
                    )
                if not inp.addSelection(entity):
                    raise AddinError("input %r takes at most %d selections" % (cid, inp._max))
            else:
                if cid in seen:
                    raise AddinError("input %r is given twice" % cid)
                inp._set_text(text)
        except core.InvalidValue as e:
            raise AddinError("input %r: %s" % (cid, e)) from None
        seen.add(cid)
        cmd._changed._fire(core.InputChangedEventArgs(cmd._changed, inp))


def _validate(cmd):
    args = core.ValidateInputsEventArgs(cmd._validate, cmd)
    cmd._validate._fire(args)
    if not args.areInputsValid:
        raise AddinError(
            "the add-in's validateInputs handler set areInputsValid = False: "
            "the inputs are not valid for command %r" % cmd._definition._id
        )
    for inp in _all_inputs(cmd._inputs):
        if isinstance(inp, core.SelectionCommandInput) and inp._visible and inp._enabled:
            n = len(inp._selections)
            if n < inp._min:
                raise AddinError(
                    "selection input %r needs at least %d selection(s), has %d (give them with "
                    "--input %s=face:...)" % (inp._id, inp._min, n, inp._id)
                )


def _preview_in_child(cmd):
    """Fire executePreview in a forked process: whatever it records is
    discarded with the process. Returns (isValidResult, problem or None)."""
    sys.stdout.flush()
    sys.stderr.flush()
    r, w = os.pipe()
    pid = os.fork()
    if pid == 0:  # child
        os.close(r)
        status = {"valid": False, "problem": None}
        try:
            args = core.CommandEventArgs(cmd._preview, cmd)
            cmd._preview._fire(args)
            status["valid"] = args.isValidResult
            rec = adsk.recorder()
            if rec.not_supported:
                status["problem"] = rec.not_supported[0]
        except BaseException as e:  # noqa: BLE001 - reported to the parent
            status["problem"] = "%s: %s" % (type(e).__name__, e)
        try:
            sys.stdout.flush()
            sys.stderr.flush()
            os.write(w, json.dumps(status).encode("utf-8"))
        finally:
            os._exit(0)
    os.close(w)
    chunks = []
    while True:
        b = os.read(r, 65536)
        if not b:
            break
        chunks.append(b)
    os.close(r)
    os.waitpid(pid, 0)
    try:
        status = json.loads(b"".join(chunks).decode("utf-8"))
    except ValueError:
        return False, "the preview process ended without a result"
    return bool(status.get("valid")), status.get("problem")


def _check(rec):
    if rec.not_supported:
        return _error(
            "not_supported", rec.not_supported[0] + " (raised inside the add-in, which caught it)"
        )
    if rec.script_error is not None:
        return _error("script", "the add-in reported an error:\n" + rec.script_error)
    return None


def execute(request):
    """Run the request (see ``rcad_fusion_bootstrap.py --addin``)."""
    rec = adsk.reset()
    try:
        base = request.get("base")
        if base:
            mod = load_module(base)
            run = getattr(mod, "run", None)
            if not callable(run):
                return _error("addin", "the base script has no run(context) function")
            run({"IsApplicationStartup": False})
            stop = getattr(mod, "stop", None)
            if callable(stop):
                stop({"IsApplicationClosing": False})
        mod = load_module(request["script"], request["addin"])
        run = getattr(mod, "run", None)
        if not callable(run):
            return _error("addin", "the add-in has no run(context) function")
        run({"IsApplicationStartup": False})
        ui = core.Application.get().userInterface

        def stop_addin():
            stop = getattr(mod, "stop", None)
            if callable(stop):
                stop({"IsApplicationClosing": False})

        if request.get("list"):
            listing = []
            for d in list(ui._definitions._items):
                cmd = _created(d) if d._created._handlers else None
                listing.append(describe(ui, d, cmd))
            stop_addin()
            return _check(rec) or {"listing": listing}

        definition = _choose(ui, request.get("command"))
        cmd = _created(definition)
        _set_inputs(cmd, request.get("inputs") or [])
        _validate(cmd)
        preview = "none"
        kept = False
        if cmd._preview._handlers:
            if not hasattr(os, "fork"):
                preview = "skipped"
            else:
                kept, problem = _preview_in_child(cmd)
                preview = "kept" if kept else "discarded"
                if problem is not None:
                    sys.stderr.write("fusion-addin: the preview failed (discarded): %s\n" % problem)
                    kept, preview = False, "failed"
        if kept:
            args = core.CommandEventArgs(cmd._preview, cmd)
            cmd._preview._fire(args)
        else:
            args = core.CommandEventArgs(cmd._execute, cmd)
            cmd._execute._fire(args)
            if args.executeFailed:
                return _error(
                    "script",
                    "the add-in's execute handler reported a failure: %s"
                    % (args.executeFailedMessage or "(no message)"),
                )
        reason = core.CommandTerminationReason.CompletedTerminationReason
        cmd._destroy._fire(core.CommandEventArgs(cmd._destroy, cmd, reason))
        stop_addin()
        problem = _check(rec)
        if problem is not None:
            return problem
        out = rec.finish()
        out["addin"] = {
            "command": definition._id,
            "preview": preview,
            "inputs": [i._describe() for i in cmd._inputs._items],
        }
        return out
    except AddinError as e:
        return _error("addin", str(e))
    except adsk.NotSupported as e:
        return _error("not_supported", str(e), traceback.format_exc())
    except BaseException as e:  # noqa: BLE001 - report everything, SystemExit included
        return _error("python", "%s: %s" % (type(e).__name__, e), traceback.format_exc())
