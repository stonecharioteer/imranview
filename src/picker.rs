use std::path::PathBuf;
use std::sync::mpsc::Sender;
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug)]
pub(crate) enum PickerRequestKind {
    OpenImage,
    CompareImage,
}

impl PickerRequestKind {
    pub(crate) fn perf_label(self) -> &'static str {
        match self {
            Self::OpenImage => "open_picker",
            Self::CompareImage => "compare_picker",
        }
    }

    fn action_label(self) -> &'static str {
        match self {
            Self::OpenImage => "open",
            Self::CompareImage => "compare",
        }
    }

    fn dialog(self) -> rfd::FileDialog {
        match self {
            Self::OpenImage => rfd::FileDialog::new().set_title("Open image").add_filter(
                "Images",
                &[
                    "avif", "bmp", "gif", "heic", "heif", "hdr", "ico", "jpeg", "jpg", "pbm",
                    "pgm", "png", "pnm", "ppm", "qoi", "tif", "tiff", "webp",
                ],
            ),
            Self::CompareImage => rfd::FileDialog::new()
                .set_title("Open compare image")
                .add_filter(
                    "Images",
                    &[
                        "avif", "bmp", "gif", "heic", "heif", "hdr", "ico", "jpeg", "jpg", "pbm",
                        "pgm", "png", "pnm", "ppm", "qoi", "tif", "tiff", "webp",
                    ],
                ),
        }
    }
}

#[derive(Debug)]
pub(crate) struct PickerResult {
    pub(crate) kind: PickerRequestKind,
    pub(crate) picked_path: Option<PathBuf>,
    pub(crate) blocked: Duration,
}

pub(crate) fn log_prepare(
    kind: PickerRequestKind,
    preferred_directory: Option<&PathBuf>,
    preferred_directory_elapsed: Duration,
) {
    let preferred_directory_label = preferred_directory
        .map(|path| path.display().to_string())
        .unwrap_or_else(|| "<none>".to_owned());

    log::debug!(
        target: "imranview::ui",
        "{} picker prepare preferred_directory={} lookup={}ms",
        kind.action_label(),
        preferred_directory_label,
        preferred_directory_elapsed.as_millis()
    );
}

pub(crate) fn launch_picker_async(
    kind: PickerRequestKind,
    preferred_directory: Option<PathBuf>,
    picker_result_tx: Sender<PickerResult>,
) {
    let preferred_directory_label = preferred_directory
        .as_ref()
        .map(|path| path.display().to_string())
        .unwrap_or_else(|| "<none>".to_owned());

    std::thread::spawn(move || {
        log::debug!(
            target: "imranview::ui",
            "{} picker invoking native dialog (no explicit set_directory) preferred_directory_candidate={}",
            kind.action_label(),
            preferred_directory_label
        );

        let picker_started = Instant::now();
        let picked_path = kind.dialog().pick_file();
        let blocked = picker_started.elapsed();

        if picker_result_tx
            .send(PickerResult {
                kind,
                picked_path,
                blocked,
            })
            .is_err()
        {
            log::warn!(
                target: "imranview::ui",
                "failed to send picker result back to UI thread"
            );
        }
    });
}
