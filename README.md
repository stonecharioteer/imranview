# ImranView

ImranView is a native, cross-platform image viewer inspired by IrfanView and built with Rust + Slint.

## Current status

This repository now contains a working first slice:

- Open an image from `File > Open...`
- View the image in a desktop window
- Navigate to `Previous` / `Next` image in the same folder
- Launch directly with a file path argument

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

See `docs/ARCHITECTURE.md` for module and roadmap details.
