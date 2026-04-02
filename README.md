# ImranView

![ImranView Favicon](assets/branding/favicon.png)

ImranView is a native, cross-platform, lightweight image viewer built with Rust + egui/eframe.

## Current status

This repository now contains a working desktop viewer slice with:

- Open an image from `File > Open...`
- Open via CLI path argument (`cargo run -- /path/to/image.jpg`)
- Previous/next navigation for images in the same folder
- Menu-first shell (compact toolbar, black canvas, segmented status bar)
- Optional thumbnail strip and dedicated thumbnail window mode, with current-image highlight
- Zoom controls: fit, actual size, in/out
- Shortcuts: `Left/Right`, `+/-`, `0` (fit), `1` (actual), `Ctrl + Mouse Wheel`, `Ctrl+S`, `Ctrl+Shift+S`
- EXIF orientation handling on load
- Preview downscaling for large images (status line shows preview vs original dimensions)
- Toolbar icons use OSS Tabler Icons (MIT)
- Background workers for open/save/edit/thumbnail tasks to keep UI responsive

Lightweight guardrails currently in place:

- Thumbnails are loaded lazily and progressively, prioritizing visible items
- Navigation preloads neighboring images to reduce next/previous latency
- Image decoding/downscaling stays in Rust background workers, UI thread remains thin

## Run locally

```bash
cargo run
```

Open a file directly:

```bash
cargo run -- /absolute/path/to/photo.jpg
```

Using the `justfile` helper:

```bash
just run --debug [--release] [/absolute/path/to/photo.jpg]
```

## Notes

- UI: egui/eframe (`src/main.rs`)
- Core logic: Rust modules (`src/app_state.rs`, `src/image_io.rs`, `src/worker.rs`)
- Supported formats come from the configured `image` crate codecs in `Cargo.toml`
- Error handling: typed IO errors with `thiserror`, contextual propagation with `anyhow`

See `docs/ARCHITECTURE.md` for module and roadmap details.
See `docs/PRD.md` for detailed product requirements and prioritized feature backlog.
