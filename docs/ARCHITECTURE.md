# ImranView Architecture

ImranView is a native desktop image viewer built with Rust + Slint.

## Design goals

- Fast startup and lightweight runtime
- Native desktop behavior on Linux, macOS, and Windows
- Clear separation between UI and image pipeline
- Easy extension toward IrfanView-like editing and batch features

## Current module layout

- `src/main.rs`
  - App bootstrap
  - Slint callback wiring
  - UI refresh logic
- `src/app_state.rs`
  - Viewer/session state
  - Current file, folder list, navigation index, directory label
  - Zoom model (`Fit` vs manual factor)
  - Thumbnail window/cache management
  - Status and window title composition
- `src/image_io.rs`
  - Decode image files via `image` crate
  - EXIF orientation application
  - Large-image preview downscaling
  - Thumbnail generation
  - Convert decoded image into Slint image buffers
  - Folder image discovery and extension filtering
- `ui/app-window.slint`
  - Main window, menu structure, folder panel, viewer canvas, thumbnail strip, status bar
  - Toolbar uses OSS Tabler SVG icons (MIT) from `assets/icons/tabler`

## Runtime flow

1. User opens image (menu or CLI file path argument).
2. `AppState::open_image()` decodes and orients the image, then discovers sibling images in the same folder.
3. If needed, large images are downscaled for interactive preview and dimensions are tracked (`preview` vs `original`).
4. Thumbnail cache is reconciled and primed around the current index.
5. UI properties (`current-image`, `status-line`, `window-title`, folder model, thumbnail model, zoom`) are updated.
6. Navigation, folder clicks, thumbnail clicks, and zoom callbacks mutate `AppState` and refresh the same view model pipeline.

## Planned expansion points

- Multi-threaded decode and prefetch cache
- Advanced transforms (resize, crop, color corrections)
- Plugin-style command registry for future tools
- Metadata panel (EXIF/IPTC/XMP)
- Batch conversion/rename pipeline

## Why this structure

The main risk in image viewers is letting rendering, IO, and state mutate each other directly. Keeping `app_state` and `image_io` separate from Slint UI gives us:

- simpler testing of decode/navigation logic
- cleaner UI iterations without touching image internals
- a path to move heavy image work off the UI thread later

## Error strategy

- `src/image_io.rs` uses typed domain errors via `thiserror` (`ImageIoError`).
- Internal decode/scan steps add rich context with `anyhow::Context`.
- Higher layers (`app_state`, `main`) use `anyhow::Result` for ergonomic propagation and user-facing status messages.
