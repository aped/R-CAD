"""``adsk.cam``: present so the usual ``import adsk.core, adsk.fusion,
adsk.cam`` works; nothing in it is supported."""

from adsk._recorder import not_in_subset


def __getattr__(name):
    if name.startswith("__"):
        raise AttributeError(name)
    raise not_in_subset("adsk.cam.%s" % name)
