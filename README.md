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
- Deep metadata inspector sections for EXIF/IPTC/XMP fields
- Edit tools: rotate/flip, resize/resample, crop, color corrections
- File operations: rename, copy/move to folder, delete with confirmation
- Batch convert/rename dialog (PNG/JPEG/WEBP/BMP/TIFF, JPEG quality)
- Batch preview summary before execution (input scan + count)
- Compare mode (load a second image and view side-by-side)
- Print current image (OS print command)
- Save dialog with format/quality and metadata policy controls
- Basic slideshow mode with keyboard start/stop and configurable interval
- Zoom controls: fit, actual size, in/out
- Shortcuts: `Cmd/Ctrl+O`, `Cmd/Ctrl+S`, `Cmd/Ctrl+Shift+S`, `Left/Right`, `+/-`, `0` (fit), `1` (actual), `Space` (slideshow), `Esc` (stop slideshow), `Cmd/Ctrl + Mouse Wheel`
- EXIF orientation handling on load
- Preview downscaling for large images (status line shows preview vs original dimensions)
- Toolbar icons use OSS Tabler Icons (MIT)
- Background workers for open/save/edit/thumbnail tasks to keep UI responsive
- Folder-navigation open actions run asynchronously on worker threads
- Batch and file operations execute on background workers
- Internal plugin host with event hooks + Plugins menu (extension API baseline)

Lightweight guardrails currently in place:

- Thumbnails are loaded lazily and progressively, prioritizing visible items
- Navigation preloads neighboring images to reduce next/previous latency
- Image decoding/downscaling stays in Rust background workers, UI thread remains thin
- Cache eviction is bounded by entry count and byte budgets (thumbnail textures + preload images)
- Runtime Performance/Cache options expose configurable cache limits and cache reset
- Optional perf-gate tooling can fail when timing warnings cross budget thresholds
- CI runs check/test, executes perf smoke (startup/open/navigation/memory), and enforces perf-gate from logs
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

## Build prerequisites

Linux (Ubuntu/Debian):

```bash
sudo apt-get update
sudo apt-get install -y \
  pkg-config \
  libglib2.0-dev \
  libgtk-3-dev \
  libxkbcommon-dev \
  libxcb-render0-dev \
  libxcb-shape0-dev \
  libxcb-xfixes0-dev
```

Windows:

- Use the MSVC Rust toolchain (`x86_64-pc-windows-msvc`).
- Install Visual Studio Build Tools with `Desktop development with C++`, MSVC toolset, and Windows 10/11 SDK (for `rc.exe` and native linker tools).
- No GTK/GLib packages are required on Windows.

Optional runtime tools (feature-dependent, all OSes):

- OCR: `tesseract`
- Lossless JPEG transform: `jpegtran`
- EXIF date/time update: `exiftool`
- Linux/macOS scan command mode: `scanimage` (SANE tools)

## Manual release pipeline

- Use GitHub Actions workflow: `Manual Release`.
- Trigger it manually from the branch you want to release (typically `main`).
- The workflow computes version as `YYYY.MM.DD.XX` (UTC date + auto-incrementing `00..99` for that date).
- It writes that version to `[package].version` in `Cargo.toml`, commits `chore(release): cut <version>`, and pushes the commit.
- It builds and publishes:
- Linux: `.AppImage`
- macOS: `.dmg`
- Windows: NSIS installer `.exe`

## Notes

- UI: egui/eframe (`src/main.rs`)
- Core logic: Rust modules (`src/app_state.rs`, `src/image_io.rs`, `src/worker.rs`)
- Supported formats come from the configured `image` crate codecs in `Cargo.toml`
- Error handling: typed IO errors with `thiserror`, contextual propagation with `anyhow`

See `docs/ARCHITECTURE.md` for module and roadmap details.
See `docs/PRD.md` for detailed product requirements and prioritized feature backlog.
