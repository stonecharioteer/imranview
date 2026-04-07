use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

use anyhow::{Result, anyhow};

use crate::image_io::{is_supported_image_path, load_image_payload};
use crate::settings::load_settings;

use super::{APP_FAVICON_PNG, ImranViewApp, load_app_icon_data};

fn percentile(sorted_values: &[f64], percentile: f64) -> f64 {
    if sorted_values.is_empty() {
        return 0.0;
    }
    let clamped = percentile.clamp(0.0, 100.0);
    let rank = ((clamped / 100.0) * sorted_values.len() as f64).ceil() as usize;
    let index = rank.saturating_sub(1).min(sorted_values.len() - 1);
    sorted_values[index]
}

fn push_slowest_sample(samples: &mut Vec<(f64, PathBuf)>, elapsed_ms: f64, path: &Path) {
    const MAX_SLOWEST: usize = 5;
    let insertion = samples
        .iter()
        .position(|(existing, _)| elapsed_ms > *existing)
        .unwrap_or(samples.len());
    if insertion < MAX_SLOWEST {
        samples.insert(insertion, (elapsed_ms, path.to_path_buf()));
        if samples.len() > MAX_SLOWEST {
            samples.pop();
        }
    } else if samples.len() < MAX_SLOWEST {
        samples.push((elapsed_ms, path.to_path_buf()));
    }
}

struct RecursiveImageScan {
    files: Vec<PathBuf>,
    directories_scanned: usize,
    scan_errors: usize,
    scan_error_samples: Vec<(PathBuf, String)>,
}

fn collect_images_recursively(root: &Path) -> RecursiveImageScan {
    const MAX_SCAN_ERROR_SAMPLES: usize = 5;

    let mut files = Vec::new();
    let mut directories_scanned = 0usize;
    let mut scan_errors = 0usize;
    let mut scan_error_samples = Vec::new();
    let mut stack = vec![root.to_path_buf()];

    while let Some(directory) = stack.pop() {
        directories_scanned += 1;
        let entries = match fs::read_dir(&directory) {
            Ok(entries) => entries,
            Err(err) => {
                scan_errors += 1;
                if scan_error_samples.len() < MAX_SCAN_ERROR_SAMPLES {
                    scan_error_samples.push((directory.clone(), err.to_string()));
                }
                continue;
            }
        };

        for entry_result in entries {
            let entry = match entry_result {
                Ok(entry) => entry,
                Err(err) => {
                    scan_errors += 1;
                    if scan_error_samples.len() < MAX_SCAN_ERROR_SAMPLES {
                        scan_error_samples.push((directory.clone(), err.to_string()));
                    }
                    continue;
                }
            };

            let path = entry.path();
            let file_type = match entry.file_type() {
                Ok(file_type) => file_type,
                Err(err) => {
                    scan_errors += 1;
                    if scan_error_samples.len() < MAX_SCAN_ERROR_SAMPLES {
                        scan_error_samples.push((path.clone(), err.to_string()));
                    }
                    continue;
                }
            };

            if file_type.is_dir() {
                stack.push(path);
            } else if file_type.is_file() && is_supported_image_path(&path) {
                files.push(path);
            }
        }
    }

    files.sort_by_key(|path| path.to_string_lossy().to_ascii_lowercase());
    RecursiveImageScan {
        files,
        directories_scanned,
        scan_errors,
        scan_error_samples,
    }
}

fn run_perf_transition(directory: PathBuf) -> Result<()> {
    if !directory.is_dir() {
        return Err(anyhow!(
            "perf-transition expects a directory, got {}",
            directory.display()
        ));
    }

    let scan = collect_images_recursively(&directory);
    let files = scan.files;
    if files.is_empty() {
        return Err(anyhow!(
            "no supported images found in {} (recursive scan)",
            directory.display()
        ));
    }

    let total_files = files.len();
    println!("perf-transition");
    println!("directory: {}", directory.display());
    println!("directories_scanned: {}", scan.directories_scanned);
    println!("scan_errors: {}", scan.scan_errors);
    println!("images_found: {total_files}");
    println!();

    let run_started = Instant::now();
    let mut decode_ms = Vec::with_capacity(total_files);
    let mut slowest = Vec::new();
    let mut failure_count = 0usize;
    let mut failure_samples: Vec<(PathBuf, String)> = Vec::new();
    let progress_interval = if total_files >= 10_000 { 1_000 } else { 250 };

    for (index, path) in files.iter().enumerate() {
        let started = Instant::now();
        match load_image_payload(path) {
            Ok(_) => {
                let elapsed_ms = started.elapsed().as_secs_f64() * 1_000.0;
                decode_ms.push(elapsed_ms);
                push_slowest_sample(&mut slowest, elapsed_ms, path);
            }
            Err(err) => {
                failure_count += 1;
                if failure_samples.len() < 5 {
                    failure_samples.push((path.clone(), err.to_string()));
                }
            }
        }

        let processed = index + 1;
        if processed % progress_interval == 0 || processed == total_files {
            eprintln!("processed {processed}/{total_files}");
        }
    }

    if decode_ms.is_empty() {
        return Err(anyhow!(
            "failed to decode every supported image in {}",
            directory.display()
        ));
    }

    decode_ms.sort_by(|left, right| left.total_cmp(right));
    let elapsed_seconds = run_started.elapsed().as_secs_f64();
    let decoded = decode_ms.len();
    println!("decoded_images: {decoded}");
    println!("failed_images: {failure_count}");
    println!("elapsed_s: {:.2}", elapsed_seconds);
    println!(
        "throughput_images_per_s: {:.2}",
        decoded as f64 / elapsed_seconds.max(0.000_001)
    );
    println!("median_ms: {:.2}", percentile(&decode_ms, 50.0));
    println!("p75_ms: {:.2}", percentile(&decode_ms, 75.0));
    println!("p90_ms: {:.2}", percentile(&decode_ms, 90.0));
    println!("p99_ms: {:.2}", percentile(&decode_ms, 99.0));

    if !slowest.is_empty() {
        println!();
        println!("slowest_samples:");
        for (elapsed_ms, path) in &slowest {
            println!("  {:.2} ms  {}", elapsed_ms, path.display());
        }
    }

    if !failure_samples.is_empty() {
        println!();
        println!("failed_samples:");
        for (path, error) in &failure_samples {
            println!("  {} => {}", path.display(), error);
        }
    }

    if !scan.scan_error_samples.is_empty() {
        println!();
        println!("scan_error_samples:");
        for (path, error) in &scan.scan_error_samples {
            println!("  {} => {}", path.display(), error);
        }
    }

    Ok(())
}

fn init_logging() {
    let env = env_logger::Env::default().default_filter_or("info");
    let mut builder = env_logger::Builder::from_env(env);
    builder.format_timestamp_millis();
    let _ = builder.try_init();
}

pub(super) fn run() -> Result<()> {
    init_logging();
    let mut args = std::env::args_os();
    let _binary = args.next();
    let first_arg = args.next();
    if first_arg.as_deref() == Some(OsStr::new("perf-transition")) {
        let Some(directory) = args.next() else {
            return Err(anyhow!(
                "usage: imranview perf-transition <DIR>\nexample: imranview perf-transition ~/Pictures/wallpaper/phone"
            ));
        };
        if args.next().is_some() {
            return Err(anyhow!(
                "usage: imranview perf-transition <DIR>\nexample: imranview perf-transition ~/Pictures/wallpaper/phone"
            ));
        }
        return run_perf_transition(PathBuf::from(directory));
    }
    let cli_path = first_arg.map(PathBuf::from);
    let startup_settings = load_settings();
    log::info!(target: "imranview::startup", "launching ImranView");

    let mut native_options = eframe::NativeOptions::default();
    if let Some([width, height]) = startup_settings.window_inner_size {
        if width > 0.0 && height > 0.0 {
            native_options.viewport = native_options.viewport.with_inner_size([width, height]);
        }
    }
    if let Some([x, y]) = startup_settings.window_position {
        native_options.viewport = native_options.viewport.with_position([x, y]);
    }
    if startup_settings.window_maximized {
        native_options.viewport = native_options.viewport.with_maximized(true);
    }
    if startup_settings.window_fullscreen {
        native_options.viewport = native_options.viewport.with_fullscreen(true);
    }
    match load_app_icon_data(APP_FAVICON_PNG) {
        Ok(icon_data) => {
            native_options.viewport = native_options.viewport.with_icon(icon_data);
        }
        Err(err) => {
            log::warn!(target: "imranview::startup", "failed to load app icon: {err:#}");
        }
    }
    eframe::run_native(
        "ImranView",
        native_options,
        Box::new(move |cc| {
            let startup_started = Instant::now();
            let app = ImranViewApp::new(cc, cli_path.clone(), startup_settings.clone());
            crate::perf::log_timing(
                "startup",
                startup_started.elapsed(),
                crate::perf::STARTUP_BUDGET,
            );
            Ok(Box::new(app))
        }),
    )
    .map_err(|err| anyhow!("failed to run egui app: {err}"))?;
    log::info!(target: "imranview::startup", "application exited");
    Ok(())
}
