# ImranView Product Requirements Document (PRD)

Version: 1.0  
Date: 2026-04-04  
Owner: ImranView core repo

## 1) Product summary

ImranView is a native, cross-platform image viewer focused on speed and low overhead.

Primary objective:
- Deliver a classic desktop image-viewer workflow with strong speed and low resource usage on Linux, macOS, and Windows.

Primary constraint:
- Stay lightweight in startup time, memory, binary footprint, and UI complexity.

## 2) Product goals

G1. Fast viewer loop:
- Open, inspect, zoom, and move to next/previous image with minimal latency.

G2. Familiar desktop workflow:
- Classic menu/toolbar/status paradigm, keyboard-centric usage, optional thumbnails workflow.

G3. Practical image utility:
- Add high-value operations (rotate, resize, crop, color corrections, batch convert/rename) without bloating runtime.

G4. Cross-platform parity:
- Same core behavior and shortcuts on Linux/macOS/Windows, with native file dialogs and packaging.

## 3) Non-goals (for now)

- Full Photoshop-class editing pipeline.
- Browser/cloud sync features.
- AI-first product positioning.
- Exact visual clone of another product's assets/icons.

## 4) Target users and core jobs

User segment A: Power users and photographers  
Jobs:
- Rapidly cull folders.
- Inspect metadata and resolution.
- Perform quick adjustments and export.

User segment B: General desktop users  
Jobs:
- Open almost any image quickly.
- Use familiar menus and shortcuts.
- Basic edits and batch conversion without complex setup.

## 5) Product principles

P1. Lightweight by default:
- No background indexing unless explicitly enabled.
- No eager full-folder thumbnail decode.
- Predictable memory ceilings.

P2. Keyboard-first:
- Every frequent action has a shortcut.
- Menu and toolbar always reflect shortcut behavior.

P3. Stable and explicit:
- Strong error messages.
- Deterministic behavior across platforms.

P4. Incremental complexity:
- Ship practical utility first.
- Delay niche features until performance budget is protected.

P5. Responsive interaction model:
- UI input handlers must stay non-blocking.
- Any expensive open/save/edit/thumbnail work runs on background workers.

## 6) Current state snapshot

Implemented:
- Open image via file dialog and CLI path.
- Next/previous navigation within folder.
- Fit/manual zoom basics (`+/-/0/1`, Ctrl+wheel).
- EXIF orientation handling on load.
- Preview downscaling for very large images.
- Optional thumbnail strip with progressive lazy thumbnail loading.
- Dedicated thumbnail window mode with folder panel (path/siblings/subfolders) and adaptive grid.
- Classic shell structure: menu, toolbar, black canvas, segmented status bar.
- Read-only metadata side panel with viewer/file details.
- macOS native app menu integration with in-window fallback.
- OSS toolbar icons (Tabler, MIT).
- Save and Save As flows.
- Save dialog with output format/quality options and metadata policy controls.
- Rotate/flip image operations.
- Resize/resample dialog + operation with interpolation choices.
- Crop dialog + operation.
- Basic color corrections dialog (brightness/contrast/gamma/saturation/grayscale).
- Centralized shortcut map shared by menu labels and keyboard handlers.
- Recent files and recent folders menu integration with persisted history.
- Basic slideshow mode (`Space` start/stop, `Esc` stop, configurable interval).
- Settings persistence for toolbar/status/thumbnail visibility, thumbnail window preferences, last-open directory, and viewport window state.
- Background workers for open/save/transform plus dedicated thumbnail worker pool.
- Background folder-open worker command for non-blocking directory navigation from thumbnail window.
- Neighbor image preload cache for low-latency next/previous navigation.
- Byte-budget memory governor for thumbnail/preload caches with deterministic eviction.
- Batch convert/rename workflow (format + quality + prefix/start index).
- Batch preview summary step before execution.
- File operations workflow (rename/copy/move/delete).
- Compare mode (load secondary image and view side-by-side).
- Print current-image workflow.
- Internal plugin extension host API (plugin events + plugin menu surface).
- Runtime performance/cache settings (configurable cache caps + cache reset).
- Startup/open/save/edit performance timing logs.
- Perf gate script to fail runs when timing warnings exceed thresholds.
- CI workflow that runs check/test and fails on perf-gate warnings.
- Perf smoke test in CI for startup/open/navigation/memory budget logging.
- Release workflow + local packaging script for Linux/macOS/Windows artifacts.
- Error strategy: `thiserror` (typed IO errors) + `anyhow` (context and propagation).

Partial:
- Metadata preservation is best-effort and currently strongest on JPEG output paths.

Missing:
- None in P0/P1 core backlog for current milestone.

## 7) Performance budgets (lightweight contract)

These budgets are release gates, not aspirational.

- Startup to first frame (no file loaded):
  - Target: <= 450 ms median on reference machine.
  - Threshold: <= 700 ms p95.
- Open 24 MP JPEG from local SSD:
  - Target: <= 150 ms median.
  - Threshold: <= 300 ms p95.
- Next/previous image switch in same folder:
  - Target: <= 90 ms median.
  - Threshold: <= 180 ms p95.
- Idle memory with no image:
  - Target: <= 80 MB RSS.
- Memory with one 24 MP image loaded:
  - Target: <= 260 MB RSS.
- Release binary size (stripped):
  - Target: <= 35 MB per platform package payload.

### 7.1) Non-negotiable responsiveness and lightweight rules

- Any feature that causes sustained UI stutter or input lag is a release blocker.
- Heavy actions (`open`, `save`, edits, thumbnail decode) must not run on the UI thread.
- Thumbnail/cache memory must be bounded with explicit caps and eviction policy.
- If any threshold budget is exceeded in validation runs, release is blocked until fixed.
- Performance regressions must be detected automatically in CI/perf smoke checks.

## 8) Feature backlog and priority

Legend:
- Priority: P0 (must), P1 (should), P2 (could)
- Status: Done, Partial, Missing

| ID | Feature | Priority | Status | Notes |
|---|---|---|---|---|
| F-001 | Classic main shell (menu/toolbar/canvas/status) | P0 | Done | Menu/toolbar/status shell + parity-oriented options wired |
| F-002 | Open image (dialog + CLI) | P0 | Done | Keep as baseline |
| F-003 | Folder navigation next/previous | P0 | Done | Needs prefetch tuning |
| F-004 | Zoom model (fit, actual, in/out) | P0 | Done | Pan anchoring improved for manual zoom transitions |
| F-005 | Segmented status bar details | P0 | Done | Includes core image stats + metadata-derived camera/capture details |
| F-006 | View toggles (toolbar/status/thumbnails) | P0 | Done | Persisted for current visibility flags |
| F-007 | Thumbnail strip (lazy/windowed) | P0 | Done | Add keyboard focus behavior |
| F-008 | Dedicated thumbnails window mode | P0 | Done | Folder panel + adaptive grid added |
| F-009 | Settings persistence (config file) | P0 | Done | View toggles + thumbnail prefs + viewport state persisted |
| F-010 | Shortcut map and conflict policy | P0 | Done | Centralized map powers handlers and menu labels |
| F-011 | Robust error surfaces | P0 | Done | User-facing error banner with quick recovery actions |
| F-012 | Startup/perf instrumentation hooks | P0 | Done | Timing logs + CI perf-gate enforcement wired |
| F-013 | Rotate left/right + flip H/V | P1 | Done | Implemented through background transform worker |
| F-014 | Resize/resample dialog | P1 | Done | Interpolation choices shipped |
| F-015 | Crop selection tools | P1 | Done | Rectangle crop dialog shipped |
| F-016 | Save / Save As pipeline | P1 | Done | Save dialog ships format/quality and metadata policy controls |
| F-017 | Color corrections (basic) | P1 | Done | Brightness/contrast/gamma/saturation/grayscale shipped |
| F-018 | Metadata panel (EXIF/IPTC/XMP) | P1 | Done | EXIF/IPTC/XMP sections and field lists shipped |
| F-019 | Recent files/recent folders | P1 | Done | Persisted recents + menu integration shipped |
| F-020 | Slideshow basic mode | P1 | Done | Keyboard and timer controls shipped |
| F-021 | Batch convert/rename | P1 | Done | Conversion/rename plus preview summary gate before run |
| F-022 | File operations (copy/move/delete/rename) | P1 | Done | Dialog-driven rename/copy/move/delete with confirmation |
| F-023 | Printing flow | P2 | Done | Print command dispatches to OS print pipeline |
| F-024 | Compare images mode | P2 | Done | Side-by-side compare mode with secondary image load |
| F-025 | Plugin extension points | P2 | Done | Internal plugin host API + event hooks + plugin menu |
| F-026 | Background command execution pipeline | P0 | Done | Open/save/edit/thumbnail work dispatched to worker threads |
| F-027 | Bounded cache and memory governor | P0 | Done | Byte-budget + count-budget eviction on preload/thumb caches |
| F-028 | Automated performance regression gate | P0 | Done | CI fails on perf warnings via perf-gate script |

## 9) Detailed requirements by epic

## Epic A: Viewer core and navigation (P0)

Requirements:
- A1: Open local files from dialog and CLI path.
- A2: Maintain deterministic file order in folder navigation.
- A3: Next/previous must wrap at boundaries.
- A4: Display current index (`x/y`) in toolbar and status.
- A5: Gracefully handle unsupported/corrupt files without crash.
- A6: Button/menu-triggered heavy operations execute asynchronously off the UI thread.

Acceptance criteria:
- AC-A1: `cargo run -- /path/file.jpg` opens directly.
- AC-A2: Folder order is stable across multiple runs.
- AC-A3: Last image next -> first image; first image previous -> last image.
- AC-A4: Counter updates within one frame after navigation.
- AC-A5: Error shown in status, app remains interactive.
- AC-A6: During open/save/edit on large images, window repaint and input remain responsive.
- AC-A7: Repeated rapid button presses do not freeze UI; stale work can be cancelled or ignored safely.

## Epic B: Zoom, pan, and viewport behavior (P0)

Requirements:
- B1: Fit mode and manual mode.
- B2: In manual mode, scrollbars appear when image exceeds viewport.
- B3: Ctrl+wheel zoom works consistently across platforms.
- B4: `0` for fit, `1` for actual-size, `+/-` for step zoom.
- B5: Preserve scroll position semantics when toggling fit/manual.

Acceptance criteria:
- AC-B1: Zoom label reflects current mode/factor.
- AC-B2: Manual zoom allows full image panning in both axes.
- AC-B3: Wheel zoom never blocks standard scroll outside Ctrl modifier.
- AC-B4: Shortcuts work with viewer focused.

## Epic C: Thumbnails workflow (P0)

Requirements:
- C1: Optional bottom thumbnail strip.
- C2: Lazy decode thumbnail cache.
- C3: Windowed prefetch near current index only.
- C4: Click thumbnail opens corresponding file.
- C5: Dedicated Thumbnails window (separate layout mode).
- C6: Thumbnail decode jobs run in cancellable background workers.

Acceptance criteria:
- AC-C1: Enabling strip does not decode all folder files immediately.
- AC-C2: Switching through large folders does not stall UI thread.
- AC-C3: Selecting thumbnail updates main viewer and status correctly.
- AC-C4: Thumbnails window supports grid and directory tree basics.
- AC-C5: Rapid scroll in thumbnails view can drop stale decode tasks without locking input.
- AC-C6: Thumbnail cache stays within configured bounds during long folder browsing sessions.

## Epic D: Classic desktop UX parity (P0/P1)

Requirements:
- D1: File/Edit/Image/Options/View/Help menus remain first-class.
- D2: Toolbar actions map 1:1 to menu actions for core commands.
- D3: Status bar is segmented and toggleable.
- D4: Persist visibility preferences for toolbar/status/thumbnails.
- D5: Keep visuals intentionally similar in structure, not in asset copying.

Acceptance criteria:
- AC-D1: No dead menu paths for P0 commands.
- AC-D2: Menu and toolbar produce identical command behavior.
- AC-D3: App relaunch restores last visibility settings.

## Epic E: Editing utilities (P1)

Requirements:
- E1: Rotate/flip.
- E2: Resize/resample.
- E3: Crop selection.
- E4: Save/Save As with format options.
- E5: Basic color correction dialog.

Acceptance criteria:
- AC-E1: Operations are non-destructive until save confirmation.
- AC-E2: Output dimensions and quality options are explicit.
- AC-E3: Saving does not silently strip essential metadata unless opted out.

## Epic F: Batch and productivity features (P1)

Requirements:
- F1: Batch convert and rename.
- F2: Recent files/folders menus.
- F3: Basic slideshow mode.
- F4: Metadata inspector.

Acceptance criteria:
- AC-F1: Batch preview summary before execution.
- AC-F2: Recent items persist across sessions.
- AC-F3: Slideshow start/stop and interval controls are keyboard accessible.

## Epic G: Reliability, packaging, and release readiness (P0/P1)

Requirements:
- G1: Cross-platform CI builds.
- G2: Basic automated tests for state and image IO.
- G3: Packaging scripts for Linux/macOS/Windows.
- G4: Crash-safe config and cache writes.
- G5: Concurrency model for UI commands is explicit and test-covered.
- G6: Performance gate automation exists for startup/open/navigation latency and memory.

Acceptance criteria:
- AC-G1: Release candidates build on all 3 platforms.
- AC-G2: Regression tests cover navigation order, wrap logic, zoom state, and error handling.
- AC-G3: User can install and launch without Rust toolchain.
- AC-G4: Tests validate command queue ordering, cancellation, and no UI deadlock under repeated commands.
- AC-G5: CI/perf checks fail build when thresholds are exceeded.

## 10) Milestone plan

Milestone M1: Fast viewer beta (2-3 weeks)
- Stabilize F-001 to F-012 with remaining partial items closed.
- Thumbnails strip and thumbnail window mode stable under large folders.
- Performance instrumentation in place with baseline measurements captured.

Milestone M2: Utility parity beta (3-5 weeks)
- Complete remaining metadata-preservation robustness and UI polish while preserving performance budgets.
- Continue UX hardening for compare/print/plugin surfaces.

Milestone M3: Public preview (2-3 weeks)
- Packaging and CI hardening.
- Cross-platform QA pass.
- Documentation and onboarding polish.

## 11) Technical strategy

- Keep decode/transforms in Rust core modules.
- Keep UI state synchronization explicit (single refresh pipeline).
- Route heavy user actions through a background command queue (worker threads + result channel).
- UI thread only dispatches commands and applies completed results.
- Maintain typed domain errors (`thiserror`) and contextual propagation (`anyhow`).

## 12) Risks and mitigations

Risk: Feature creep breaks lightweight objective.  
Mitigation: Enforce performance budgets as milestone gates.

Risk: Cross-platform UI behavior drift.  
Mitigation: Platform matrix smoke tests for shortcuts, wheel, dialogs.

Risk: Thumbnail and batch operations spike memory.  
Mitigation: Bounded cache, streaming pipeline, explicit memory limits.

Risk: Menu complexity grows faster than implementation.  
Mitigation: Mark stubs clearly and prioritize high-frequency actions first.

## 13) Definition of done (for major features)

A feature is done only when all conditions hold:
- Behavior implemented and reachable from menu/shortcut (and toolbar if applicable).
- Error paths handled with clear user feedback.
- Unit/integration tests added where practical.
- Measured against relevant performance budget.
- Verified to keep UI responsive under repeated/rapid user actions.
- Memory and cache behavior validated against configured caps.
- Documented in README/changelog.

## 14) Immediate execution checklist

- [x] Build dedicated Thumbnails window mode baseline (list mode).
- [x] Upgrade Thumbnails window mode with grid + directory tree baseline.
- [x] Add persistent settings file for toolbar/status/thumbnails visibility and last-open directory.
- [x] Expand settings persistence to include last window state and advanced preferences.
- [x] Implement rotate/flip commands.
- [x] Implement Resize/Resample dialog and operation.
- [x] Expand Save/Save As with richer format options and metadata controls.
- [x] Add recent files/recent folders.
- [x] Add performance timing logs for startup/open/navigation/save/edit.
- [x] Add tests for navigation wrap, zoom state transitions, and error recovery.
- [x] Implement background command queue so open/save/edit/thumbnail decode do not block UI input.
- [x] Add full bounded thumbnail/decode cache policy with configurable limits and stronger eviction controls.
- [x] Add CI perf smoke checks that gate on startup/open/navigation/memory thresholds.
