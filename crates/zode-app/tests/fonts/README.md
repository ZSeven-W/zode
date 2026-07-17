# Deterministic screenshot fonts

The screenshot suite derives two test-only subsets from **Noto Sans SC** so
Chinese and Latin shaping do not depend on fonts installed on the runner.
Their internal family is renamed to the repository-private
**Zode Snapshot Sans SC**, preventing an installed copy of Noto Sans SC from
taking precedence over the bundled faces. They are not application release
assets. Two keyboard shortcut symbols are completed from **Noto Sans Symbols
2**: U+2325 uses its original outline, while U+2303 is a deterministic cmap
alias to Noto Sans SC's ASCII circumflex.

## Source and license

- Upstream: [google/fonts `ofl/notosanssc`](https://github.com/google/fonts/tree/ec0464b978de222073645d6d3366f3fdf03376d8/ofl/notosanssc)
- Source commit: `ec0464b978de222073645d6d3366f3fdf03376d8`
- Source file: `NotoSansSC[wght].ttf`
- Source SHA-256: `a3041811a78c361b1de50f953c805e0244951c21c5bd412f7232ef0d899af0da`
- Symbol source: [google/fonts `ofl/notosanssymbols2`](https://github.com/google/fonts/tree/ec0464b978de222073645d6d3366f3fdf03376d8/ofl/notosanssymbols2)
- Symbol source file: `NotoSansSymbols2-Regular.ttf`
- Symbol source SHA-256: `7d5fb73b7ca67a6798101741f5d280a3d016a56a197afcd4199dbb57b4b82a21`
- Symbol copyright: `Copyright 2022 The Noto Project Authors
  (https://github.com/notofonts/symbols)`
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
curl -fL \
  'https://raw.githubusercontent.com/google/fonts/ec0464b978de222073645d6d3366f3fdf03376d8/ofl/notosanssymbols2/NotoSansSymbols2-Regular.ttf' \
  -o /tmp/NotoSansSymbols2-Regular-ec0464b.ttf

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

python3 crates/zode-app/tests/fonts/augment-symbols.py \
  crates/zode-app/tests/fonts/NotoSansSC-Regular.subset.ttf \
  /tmp/NotoSansSymbols2-Regular-ec0464b.ttf
python3 crates/zode-app/tests/fonts/augment-symbols.py \
  crates/zode-app/tests/fonts/NotoSansSC-SemiBold.subset.ttf \
  /tmp/NotoSansSymbols2-Regular-ec0464b.ttf
```

Expected SHA-256 values:

```text
d3ab43f842e2e767cb725be66a7d4b6ca3471ac9a224d0d945416c1878fbb8ea  NotoSansSC-Regular.subset.ttf
2e76b234fc752bb51843f661fb12c2ed9a76129a0a9b820ce6a31db6ccdf0fb9  NotoSansSC-SemiBold.subset.ttf
```

When visible copy changes, update `glyphs.txt`, rebuild both subsets, and run
the snapshot update flow. Keep the source commit and all hashes in this file
in sync with the generated assets.
