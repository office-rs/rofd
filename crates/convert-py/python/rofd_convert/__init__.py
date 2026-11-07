"""rofd-convert — headless OFD to PDF conversion (convert spec §6.6).

Public surface re-exported from the native module. Pool recipe (spec
D15-safe shape): one Converter per thread via threading.local — see
demo/run.py for the full ThreadPoolExecutor example.

    from rofd_convert import Converter, register_font_file
    register_font_file("simsun.ttc")
    c = Converter()
    pdf, warnings = c.convert(ofd_bytes)
"""

from .rofd_convert_native import (
    ConvertError,
    Converter,
    has_default_font,
    register_default_font,
    register_font,
    register_font_file,
    registered_families,
)

__all__ = [
    "ConvertError",
    "Converter",
    "has_default_font",
    "register_default_font",
    "register_font",
    "register_font_file",
    "registered_families",
]

try:
    from importlib.metadata import version as _version

    __version__ = _version("rofd-convert")
except Exception:  # pragma: no cover - dev layouts without installed dist-info
    from . import rofd_convert_native as _native

    __version__ = getattr(_native, "__version__", "0.0.0")
