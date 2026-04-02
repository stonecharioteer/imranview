# ImranView

ImranView is a native, cross-platform image viewer inspired by IrfanView and built with Rust + Slint.

## Current status

This repository now contains a working desktop viewer slice with:

- Open an image from `File > Open...`
- Open via CLI path argument (`cargo run -- /path/to/image.jpg`)
- Previous/next navigation for images in the same folder
- Classic IrfanView-inspired shell (menu-first, compact toolbar, black canvas, segmented status bar)
- Optional thumbnail strip with current-image highlight
- Zoom controls: fit, actual size, in/out
- Shortcuts: `Left/Right`, `+/-`, `0` (fit), `1` (actual), `Ctrl + Mouse Wheel`, `T`/`H`/`S` for thumbnails/toolbar/status
- EXIF orientation handling on load
- Preview downscaling for large images (status line shows preview vs original dimensions)
- Toolbar icons use OSS Tabler Icons (MIT), not IrfanView assets

Lightweight guardrails currently in place:

- Thumbnails are not generated until the strip is enabled
- Thumbnail loading is windowed around the current file (no full-folder eager decode)
- Image decoding/downscaling is kept in the Rust core, UI stays thin

## Run locally

```bash
cargo run
```

Open a file directly:

```bash
cargo run -- /absolute/path/to/photo.jpg
```

## Notes

- UI: Slint (`ui/app-window.slint`)
- Core logic: Rust modules (`src/app_state.rs`, `src/image_io.rs`)
- Supported formats come from the configured `image` crate codecs in `Cargo.toml`
- Error handling: typed IO errors with `thiserror`, contextual propagation with `anyhow`

See `docs/ARCHITECTURE.md` for module and roadmap details.
See `docs/PRD.md` for detailed product requirements and prioritized feature backlog.
