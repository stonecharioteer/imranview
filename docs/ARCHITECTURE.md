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
  - Current file, folder list, navigation index
  - Status and window title composition
- `src/image_io.rs`
  - Decode image files via `image` crate
  - Convert decoded image into Slint image buffers
  - Folder image discovery and extension filtering
- `ui/app-window.slint`
  - Main window, menu structure, canvas, status bar

## Runtime flow

1. User opens image (menu or CLI file path argument).
2. `AppState::open_image()` decodes the image and discovers sibling images in the same folder.
3. UI properties (`current-image`, `status-line`, `window-title`) are updated.
4. `Next/Previous` navigation advances through the discovered folder list.

## Planned expansion points

- Zoom model and fit/actual-size modes
- EXIF orientation handling
- Thumbnail strip and folder browser
- Multi-threaded decode and prefetch cache
- Advanced transforms (resize, crop, color corrections)
- Plugin-style command registry for future tools

## Why this structure

The main risk in image viewers is letting rendering, IO, and state mutate each other directly. Keeping `app_state` and `image_io` separate from Slint UI gives us:

- simpler testing of decode/navigation logic
- cleaner UI iterations without touching image internals
- a path to move heavy image work off the UI thread later
