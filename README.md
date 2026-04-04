# ImranView

<img src="assets/branding/favicon.png" alt="ImranView Favicon" width="160" />

ImranView is a native, cross-platform, lightweight image viewer built with Rust + egui/eframe.

## Current status

This repository now contains a working desktop viewer slice with:

- Open an image from `File > Open...`
- Open via CLI path argument (`cargo run -- /path/to/image.jpg`)
- Previous/next navigation for images in the same folder
- Menu-first shell (compact toolbar, black canvas, segmented status bar)
- Optional thumbnail strip and dedicated thumbnail window mode with folder panel + adaptive grid
- Recent files/recent folders in the File menu
- Metadata panel (read-only viewer/session + file metadata)
- Edit tools: rotate/flip, resize/resample, crop, color corrections
- File operations: rename, copy/move to folder, delete with confirmation
- Batch convert/rename dialog (PNG/JPEG/WEBP/BMP/TIFF, JPEG quality)
- Basic slideshow mode with keyboard start/stop and configurable interval
- Zoom controls: fit, actual size, in/out
- Shortcuts: `Cmd/Ctrl+O`, `Cmd/Ctrl+S`, `Cmd/Ctrl+Shift+S`, `Left/Right`, `+/-`, `0` (fit), `1` (actual), `Space` (slideshow), `Esc` (stop slideshow), `Cmd/Ctrl + Mouse Wheel`
- EXIF orientation handling on load
- Preview downscaling for large images (status line shows preview vs original dimensions)
- Toolbar icons use OSS Tabler Icons (MIT)
- Background workers for open/save/edit/thumbnail tasks to keep UI responsive
- Folder-navigation open actions run asynchronously on worker threads
- Batch and file operations execute on background workers

Lightweight guardrails currently in place:

- Thumbnails are loaded lazily and progressively, prioritizing visible items
- Navigation preloads neighboring images to reduce next/previous latency
- Image decoding/downscaling stays in Rust background workers, UI thread remains thin
- Cache eviction is bounded by entry count and byte budgets (thumbnail textures + preload images)
- Optional perf-gate tooling can fail when timing warnings cross budget thresholds
- CI runs check/test and enforces perf-gate from captured logs
- Cross-platform release packaging workflow publishes Linux/macOS/Windows artifacts on version tags

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

just perf-gate [debug.log ...]

just ci

just package [target]
```

## Notes

- UI: egui/eframe (`src/main.rs`)
- Core logic: Rust modules (`src/app_state.rs`, `src/image_io.rs`, `src/worker.rs`)
- Supported formats come from the configured `image` crate codecs in `Cargo.toml`
- Error handling: typed IO errors with `thiserror`, contextual propagation with `anyhow`

See `docs/ARCHITECTURE.md` for module and roadmap details.
See `docs/PRD.md` for detailed product requirements and prioritized feature backlog.
