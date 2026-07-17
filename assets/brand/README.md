# Zode desktop brand assets

The canonical artwork remains `zode-logo.png` (with `zode-logo.svg` as its
editable vector source). Task 17 adds three format conversions derived from
that exact logo, without changing the artwork:

- `zode-512.png`: 512 px Linux desktop and runtime-window icon.
- `zode.icns`: macOS application-bundle icon with 16–1024 px representations.
- `zode.ico`: 256 px Windows executable and installer icon.

The derivatives were generated locally from `zode-logo.png`: macOS PNG sizes
were resized with `sips` and assembled with `iconutil`; the ICO was encoded by
FFmpeg. They contain no Codex/OpenAI brand assets.
