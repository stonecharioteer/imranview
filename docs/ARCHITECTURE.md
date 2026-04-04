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
  - Edit dialogs (resize/crop/color corrections)
  - Save dialog with format/quality/metadata controls
  - Batch convert/rename preview + execution dialogs
  - File-operation dialogs + print/compare entry points
  - Folder-panel + thumbnail-grid window mode
  - Compare mode (side-by-side primary/secondary image)
  - Recent-file/folder commands
  - Basic slideshow tick scheduler
  - Metadata side panel with EXIF/IPTC/XMP sections
  - Plugins menu backed by internal plugin host
  - Performance/cache settings dialog and live cache-policy updates
  - Window viewport state sync (position/size/maximized/fullscreen)
- `src/native_menu.rs` (macOS only)
  - Native application menu bar installation (`muda`)
  - Menu event polling and action mapping
  - Native menu item checked/enabled state sync (including print/compare hooks)
- `src/plugin.rs`
  - Internal plugin host API (`ViewerPlugin`, `PluginEvent`, `PluginContext`)
  - Built-in event-counter sample plugin
- `src/shortcuts.rs`
  - Centralized shortcut map for menu labels and input handlers
  - Cross-platform `Cmd`/`Ctrl` command shortcut handling
- `src/app_state.rs`
  - Viewer/session state
  - Current file, folder list, navigation index, directory label
  - Zoom model (`Fit` vs manual factor)
  - Thumbnail list/window mode state and decode hints
  - Persisted thumbnail-window UI preferences (sidebar width + card size)
  - Persisted recent files/folders and slideshow interval
  - Metadata-panel visibility preference
  - Persisted viewport/window state used during startup restore
  - Status and window title composition
- `src/image_io.rs`
  - Decode image files via `image` crate
  - EXIF orientation application
  - Large-image preview downscaling
  - Thumbnail generation
  - Folder image discovery and extension filtering
- `src/worker.rs`
  - Background worker threads for open/save/transform/print/compare operations
  - Batch convert/rename command execution
  - File operations (rename/copy/move/delete)
  - Metadata extraction during open/compare load
  - Runtime preload cache policy updates (`UpdateCachePolicy`)
  - Background folder-open operation (`OpenDirectory`)
  - Dedicated thumbnail worker pool
  - Neighbor preload cache for low-latency next/previous navigation with memory-budget eviction
  - Command/result channel contract (`WorkerCommand`, `WorkerResult`)
- `src/settings.rs`
  - Persistent viewer settings load/save
  - Cross-platform config path resolution
- `src/perf.rs`
  - Performance budget definitions
  - Structured timing logs for startup/open/save/edit paths
- `scripts/perf_gate.sh`
  - Regression gate helper that fails when perf warnings exceed thresholds in one or more logs
- `tests/perf_smoke.rs`
  - CI smoke test for startup/open/navigation timings and memory budget logging
- `scripts/package_release.sh`
  - Local packaging helper for host or explicit Rust target triples
- `.github/workflows/ci.yml`
  - Runs `cargo check`, `cargo test` (including perf smoke), and perf-gate enforcement on captured logs
- `.github/workflows/release.yml`
  - Cross-platform release build and artifact publishing for version tags

## Runtime flow

1. User opens image (menu or CLI file path argument).
2. UI thread enqueues heavy work (`OpenImage`, `SaveImage`, `TransformImage`) to worker channels.
3. Worker decodes/orients image and discovers sibling files in folder.
4. App state applies worker results and refreshes main texture on the next UI frame.
5. Thumbnail cards request lazy decode only when hinted/visible; decode runs in thumbnail workers.
6. Neighbor images are preloaded asynchronously to speed up rapid next/previous navigation.
7. Folder panel entries are cached from current directory and opened asynchronously.
8. Status bar and window title are recomputed from `AppState` after each state mutation.
9. Plugin host receives lifecycle events (open/save/edit/batch/file/compare/print) without coupling plugin code to core UI internals.

## Planned expansion points

- Advanced compare/inspection variants (split slider, diff overlays)
- Deep metadata decoding beyond baseline EXIF/IPTC/XMP extraction breadth
- External plugin loading/discovery (internal API is already in place)

## Why this structure

The main risk in image viewers is letting rendering, IO, and state mutate each other directly. Keeping `app_state` and `image_io` separate from UI code gives us:

- simpler testing of decode/navigation logic
- cleaner UI iterations without touching image internals
- background execution of heavy image work while keeping UI responsive

## Error strategy

- `src/image_io.rs` uses typed domain errors via `thiserror` (`ImageIoError`).
- Internal decode/scan steps add rich context with `anyhow::Context`.
- Higher layers (`app_state`, `main`) use `anyhow::Result` for ergonomic propagation and user-facing status messages.
