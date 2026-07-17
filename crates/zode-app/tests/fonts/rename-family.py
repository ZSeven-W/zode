#!/usr/bin/env python3
"""Give a generated snapshot font a repository-private family name."""

from __future__ import annotations

import argparse
from pathlib import Path

from fontTools.ttLib import TTFont


FAMILY = "Zode Snapshot Sans SC"


def rename_family(path: Path, style: str) -> None:
    font = TTFont(path, recalcTimestamp=False)
    names = font["name"]
    postscript_style = style.replace(" ", "")
    replacements = {
        1: FAMILY,
        2: style,
        3: f"ZodeSnapshotSansSC-{postscript_style};2.004",
        4: f"{FAMILY} {style}",
        6: f"ZodeSnapshotSansSC-{postscript_style}",
        16: FAMILY,
        17: style,
        25: "ZodeSnapshotSansSC",
    }
    names.names = [record for record in names.names if record.nameID not in replacements]
    for name_id, value in replacements.items():
        names.setName(value, name_id, 3, 1, 0x409)

    temporary = path.with_suffix(f"{path.suffix}.tmp")
    font.save(temporary, reorderTables=False)
    temporary.replace(path)


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("font", type=Path)
    parser.add_argument("style", choices=("Regular", "SemiBold"))
    args = parser.parse_args()
    rename_family(args.font, args.style)


if __name__ == "__main__":
    main()
