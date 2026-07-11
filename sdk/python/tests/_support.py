"""Shared test support: make the src-layout package importable and give
scripted-child tests collision-free temp binaries.

The verification command runs ``python3 -m unittest discover -s tests`` from
``sdk/python`` without ``PYTHONPATH`` set, so each test module imports this
module first to prepend ``../src`` onto ``sys.path``.
"""

from __future__ import annotations

import os
import sys
import tempfile

_SRC = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "src")
if _SRC not in sys.path:
    sys.path.insert(0, os.path.abspath(_SRC))

# Module-level counter (NOT time.time()) so parallel/back-to-back scripted
# children never collide on a directory name.
_counter = 0


def scripted_child(body: str) -> str:
    """Write an executable ``/bin/sh`` stub with a unique path and return it."""
    global _counter
    _counter += 1
    directory = tempfile.mkdtemp(prefix=f"zode-py-{os.getpid()}-{_counter}-")
    path = os.path.join(directory, "zode")
    with open(path, "w", encoding="utf-8") as handle:
        handle.write(f"#!/bin/sh\n{body}\n")
    os.chmod(path, 0o755)
    return path
