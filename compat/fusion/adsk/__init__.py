"""R-CAD's implementation of a subset of the Fusion add-in API surface.

Scripts written for that API (``import adsk.core, adsk.fusion``) run under
``rcad-cli fusion-script SCRIPT.py``. Every call is recorded as R-CAD
commands (see ``adsk._recorder``) which the CLI then executes. Anything
outside the documented subset (docs/FUSION_SCRIPTS.md) raises
:class:`NotSupported` naming the member.

This is an independent implementation written from the public API
reference; it is not affiliated with or endorsed by Autodesk.
"""

from adsk._recorder import NotSupported, recorder, reset  # noqa: F401


def autoTerminate(value=True):
    """Accepted for compatibility; a script always ends when ``run`` returns."""
    return True


def terminate():
    """Accepted for compatibility; nothing to do."""
    return True


def doEvents():
    """Accepted for compatibility; there is no event loop."""
    return True
