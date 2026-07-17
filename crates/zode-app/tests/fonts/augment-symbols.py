#!/usr/bin/env python3
"""Add deterministic shortcut symbols to a Zode snapshot font subset."""

from __future__ import annotations

import argparse
import copy
import hashlib
import os
import tempfile
from pathlib import Path

from fontTools.ttLib import TTFont


SYMBOLS_SHA256 = "7d5fb73b7ca67a6798101741f5d280a3d016a56a197afcd4199dbb57b4b82a21"
CONTROL = 0x2303
OPTION = 0x2325
ASCII_CIRCUMFLEX = 0x005E


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def add_symbols(target_path: Path, symbols_path: Path) -> None:
    actual_hash = sha256(symbols_path)
    if actual_hash != SYMBOLS_SHA256:
        raise SystemExit(
            f"unexpected Noto Sans Symbols 2 hash: {actual_hash}; "
            f"expected {SYMBOLS_SHA256}"
        )

    target = TTFont(target_path, recalcTimestamp=False)
    symbols = TTFont(symbols_path, recalcTimestamp=False)
    target_cmap = target.getBestCmap()
    symbols_cmap = symbols.getBestCmap()
    caret_name = target_cmap.get(ASCII_CIRCUMFLEX)
    option_source_name = symbols_cmap.get(OPTION)
    if caret_name is None:
        raise SystemExit(f"{target_path} has no U+005E glyph to alias as U+2303")
    if option_source_name is None:
        raise SystemExit(f"{symbols_path} has no U+2325 glyph")
    if symbols["glyf"][option_source_name].isComposite():
        raise SystemExit("U+2325 must remain a self-contained simple glyph")
    if target["head"].unitsPerEm != symbols["head"].unitsPerEm:
        raise SystemExit("target and symbol fonts must use the same units per em")

    option_name = "uni2325"
    glyph_order = target.getGlyphOrder()
    if option_name not in glyph_order:
        target.setGlyphOrder([*glyph_order, option_name])
    target["glyf"][option_name] = copy.deepcopy(symbols["glyf"][option_source_name])
    target["hmtx"][option_name] = symbols["hmtx"][option_source_name]
    if "vmtx" in target and "vmtx" in symbols:
        target["vmtx"][option_name] = symbols["vmtx"][option_source_name]

    unicode_tables = [
        table
        for table in target["cmap"].tables
        if table.isUnicode() and table.format in (4, 12)
    ]
    if not unicode_tables:
        raise SystemExit(f"{target_path} has no Unicode cmap")
    for table in unicode_tables:
        table.cmap[CONTROL] = caret_name
        table.cmap[OPTION] = option_name

    # OpenType Unicode-range bit 39 covers Miscellaneous Technical. Preserve
    # every other range bit from the pinned subset instead of broadly
    # recalculating historical metadata.
    target["OS/2"].ulUnicodeRange2 |= 1 << 7
    with tempfile.NamedTemporaryFile(
        prefix=f".{target_path.name}.", suffix=".tmp", dir=target_path.parent, delete=False
    ) as output:
        output_path = Path(output.name)
    try:
        target.save(output_path, reorderTables=False)
        os.replace(output_path, target_path)
    finally:
        output_path.unlink(missing_ok=True)


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("target", type=Path)
    parser.add_argument("symbols", type=Path)
    args = parser.parse_args()
    add_symbols(args.target, args.symbols)


if __name__ == "__main__":
    main()
