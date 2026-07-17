# Deterministic screenshot fonts

The screenshot suite bundles two test-only subsets of **Noto Sans SC** so
Chinese and Latin shaping do not depend on fonts installed on the runner.
They are not application release assets.

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
```

Expected SHA-256 values:

```text
b51ff8c2564d9f0d645e874870f17376907117f94bde7c79b7ff9707c2a1cf7f  NotoSansSC-Regular.subset.ttf
2ec6309d0760919ded4df04effa717ea2a23fc05635feaa4910dfb38d018d55b  NotoSansSC-SemiBold.subset.ttf
```

When visible copy changes, update `glyphs.txt`, rebuild both subsets, and run
the snapshot update flow. Keep the source commit and all hashes in this file
in sync with the generated assets.
