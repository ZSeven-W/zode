# Deterministic screenshot fonts

The screenshot suite derives two test-only subsets from **Noto Sans SC** so
Chinese and Latin shaping do not depend on fonts installed on the runner.
Their internal family is renamed to the repository-private
**Zode Snapshot Sans SC**, preventing an installed copy of Noto Sans SC from
taking precedence over the bundled faces. They are not application release
assets.

## Source and license

- Upstream: [google/fonts `ofl/notosanssc`](https://github.com/google/fonts/tree/ec0464b978de222073645d6d3366f3fdf03376d8/ofl/notosanssc)
- Source commit: `ec0464b978de222073645d6d3366f3fdf03376d8`
- Source file: `NotoSansSC[wght].ttf`
- Source SHA-256: `a3041811a78c361b1de50f953c805e0244951c21c5bd412f7232ef0d899af0da`
- License: SIL Open Font License 1.1; the exact upstream text is copied to
  [`LICENSE.txt`](LICENSE.txt).

The variable source is instantiated at weights 400 and 600, then subset to
the exact checked-in [`glyphs.txt`](glyphs.txt). The glyph file includes the
printable ASCII range and every non-ASCII character currently painted by the
snapshot fixture and desktop UI surfaces under test.

## Rebuild

The checked-in files were generated with fontTools `4.39.4`. From the
repository root, with `fonttools` and `pyftsubset` on `PATH`, run:

```bash
curl -fL \
  'https://raw.githubusercontent.com/google/fonts/ec0464b978de222073645d6d3366f3fdf03376d8/ofl/notosanssc/NotoSansSC%5Bwght%5D.ttf' \
  -o /tmp/NotoSansSC-wght-ec0464b.ttf

fonttools varLib.instancer /tmp/NotoSansSC-wght-ec0464b.ttf \
  wght=400 --update-name-table --no-recalc-timestamp \
  -o /tmp/NotoSansSC-Regular.ttf
fonttools varLib.instancer /tmp/NotoSansSC-wght-ec0464b.ttf \
  wght=600 --update-name-table --no-recalc-timestamp \
  -o /tmp/NotoSansSC-SemiBold.ttf

pyftsubset /tmp/NotoSansSC-Regular.ttf \
  --output-file=crates/zode-app/tests/fonts/NotoSansSC-Regular.subset.ttf \
  --text-file=crates/zode-app/tests/fonts/glyphs.txt \
  --layout-features='*' --glyph-names --symbol-cmap --legacy-cmap \
  --notdef-glyph --notdef-outline --recommended-glyphs \
  --name-IDs='*' --name-legacy --name-languages='*' --canonical-order
pyftsubset /tmp/NotoSansSC-SemiBold.ttf \
  --output-file=crates/zode-app/tests/fonts/NotoSansSC-SemiBold.subset.ttf \
  --text-file=crates/zode-app/tests/fonts/glyphs.txt \
  --layout-features='*' --glyph-names --symbol-cmap --legacy-cmap \
  --notdef-glyph --notdef-outline --recommended-glyphs \
  --name-IDs='*' --name-legacy --name-languages='*' --canonical-order

python3 crates/zode-app/tests/fonts/rename-family.py \
  crates/zode-app/tests/fonts/NotoSansSC-Regular.subset.ttf Regular
python3 crates/zode-app/tests/fonts/rename-family.py \
  crates/zode-app/tests/fonts/NotoSansSC-SemiBold.subset.ttf SemiBold
```

Expected SHA-256 values:

```text
65e7fdbf0e116e31ac8480dc154ad384463beb7aac01615d8150af52269057d9  NotoSansSC-Regular.subset.ttf
1f9e3b68d8c37914aa76373a0d12c93b786b2b50cac4e9f8fd8756c4fa5196db  NotoSansSC-SemiBold.subset.ttf
```

When visible copy changes, update `glyphs.txt`, rebuild both subsets, and run
the snapshot update flow. Keep the source commit and all hashes in this file
in sync with the generated assets.
