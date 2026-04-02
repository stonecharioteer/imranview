# ImranView Architecture

ImranView is a native desktop image viewer built with Rust + egui/eframe.

## Design goals

- Fast startup and lightweight runtime
- Native desktop behavior on Linux, macOS, and Windows
- Clear separation between UI and image pipeline
- Easy extension toward practical editing and batch workflows

## Current module layout

- `src/main.rs`
  - App bootstrap
  - egui render/update loop
  - Toolbar/menu/status/thumbnail UI composition
  - Keyboard shortcut handling
  - Dispatch of commands to background workers
- `src/native_menu.rs` (macOS only)
  - Native application menu bar installation (`muda`)
  - Menu event polling and action mapping
  - Native menu item checked/enabled state sync
- `src/app_state.rs`
  - Viewer/session state
  - Current file, folder list, navigation index, directory label
  - Zoom model (`Fit` vs manual factor)
  - Thumbnail list/window mode state and decode hints
  - Status and window title composition
- `src/image_io.rs`
  - Decode image files via `image` crate
  - EXIF orientation application
  - Large-image preview downscaling
  - Thumbnail generation
  - Folder image discovery and extension filtering
- `src/worker.rs`
  - Background worker threads for open/save/transform operations
  - Dedicated thumbnail worker pool
  - Neighbor preload cache for low-latency next/previous navigation
  - Command/result channel contract (`WorkerCommand`, `WorkerResult`)
- `src/settings.rs`
  - Persistent viewer settings load/save
  - Cross-platform config path resolution
- `src/perf.rs`
  - Performance budget definitions
  - Structured timing logs for startup/open/save/edit paths

## Runtime flow

1. User opens image (menu or CLI file path argument).
2. UI thread enqueues heavy work (`OpenImage`, `SaveImage`, `TransformImage`) to worker channels.
3. Worker decodes/orients image and discovers sibling files in folder.
4. App state applies worker results and refreshes main texture on the next UI frame.
5. Thumbnail cards request lazy decode only when hinted/visible; decode runs in thumbnail workers.
6. Neighbor images are preloaded asynchronously to speed up rapid next/previous navigation.
7. Status bar and window title are recomputed from `AppState` after each state mutation.

## Planned expansion points

- Advanced transforms (resize, crop, color corrections)
- Metadata panel (EXIF/IPTC/XMP)
- Batch conversion/rename pipeline
- CI performance regression gates

## Why this structure

The main risk in image viewers is letting rendering, IO, and state mutate each other directly. Keeping `app_state` and `image_io` separate from UI code gives us:

- simpler testing of decode/navigation logic
- cleaner UI iterations without touching image internals
- background execution of heavy image work while keeping UI responsive

## Error strategy

- `src/image_io.rs` uses typed domain errors via `thiserror` (`ImageIoError`).
- Internal decode/scan steps add rich context with `anyhow::Context`.
- Higher layers (`app_state`, `main`) use `anyhow::Result` for ergonomic propagation and user-facing status messages.
