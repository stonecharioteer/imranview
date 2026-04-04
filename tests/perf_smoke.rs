use std::time::{Duration, Instant};

#[cfg(target_os = "linux")]
use std::fs;

use image::{DynamicImage, Rgba, RgbaImage};
use tempfile::tempdir;

#[derive(Clone, Copy)]
struct PerfBudget {
    target_ms: u128,
    threshold_ms: u128,
}

const STARTUP_BUDGET: PerfBudget = PerfBudget {
    target_ms: 450,
    threshold_ms: 700,
};
const OPEN_BUDGET: PerfBudget = PerfBudget {
    target_ms: 150,
    threshold_ms: 300,
};
const NAVIGATION_BUDGET: PerfBudget = PerfBudget {
    target_ms: 90,
    threshold_ms: 180,
};

fn log_timing(label: &str, elapsed: Duration, budget: PerfBudget) {
    let elapsed_ms = elapsed.as_millis();
    if elapsed_ms > budget.threshold_ms {
        log::warn!(
            target: "imranview::perf",
            "[WARN] {label}={}ms (target={}ms threshold={}ms)",
            elapsed_ms,
            budget.target_ms,
            budget.threshold_ms
        );
    } else if elapsed_ms > budget.target_ms {
        log::info!(
            target: "imranview::perf",
            "[SLOW] {label}={}ms (target={}ms threshold={}ms)",
            elapsed_ms,
            budget.target_ms,
            budget.threshold_ms
        );
    } else {
        log::debug!(
            target: "imranview::perf",
            "[OK] {label}={}ms (target={}ms threshold={}ms)",
            elapsed_ms,
            budget.target_ms,
            budget.threshold_ms
        );
    }
}

fn log_memory_budget(label: &str, bytes: u64, target_mb: u64, threshold_mb: u64) {
    let measured_mb = bytes / (1024 * 1024);
    if measured_mb > threshold_mb {
        log::warn!(
            target: "imranview::perf",
            "[WARN] {label}={}MB (target={}MB threshold={}MB)",
            measured_mb,
            target_mb,
            threshold_mb
        );
    } else if measured_mb > target_mb {
        log::info!(
            target: "imranview::perf",
            "[SLOW] {label}={}MB (target={}MB threshold={}MB)",
            measured_mb,
            target_mb,
            threshold_mb
        );
    } else {
        log::debug!(
            target: "imranview::perf",
            "[OK] {label}={}MB (target={}MB threshold={}MB)",
            measured_mb,
            target_mb,
            threshold_mb
        );
    }
}

fn current_rss_bytes() -> Option<u64> {
    #[cfg(target_os = "linux")]
    {
        let status = fs::read_to_string("/proc/self/status").ok()?;
        let line = status.lines().find(|line| line.starts_with("VmRSS:"))?;
        let kb = line
            .split_whitespace()
            .nth(1)
            .and_then(|value| value.parse::<u64>().ok())?;
        Some(kb * 1024)
    }

    #[cfg(not(target_os = "linux"))]
    {
        None
    }
}

#[test]
fn perf_smoke_startup_open_navigation_and_memory() {
    let startup_started = Instant::now();
    let _placeholder_startup_object = vec![0u8; 32];
    log_timing("startup", startup_started.elapsed(), STARTUP_BUDGET);

    if let Some(idle_rss) = current_rss_bytes() {
        log_memory_budget("idle_memory", idle_rss, 80, 120);
    }

    let dir = tempdir().expect("failed to create temp dir");
    let image_a = dir.path().join("image_a.jpg");
    let image_b = dir.path().join("image_b.jpg");

    let large_a = RgbaImage::from_pixel(6000, 4000, Rgba([24, 38, 52, 255]));
    DynamicImage::ImageRgba8(large_a)
        .save(&image_a)
        .expect("failed to create image_a");
    let large_b = RgbaImage::from_pixel(6000, 4000, Rgba([42, 62, 82, 255]));
    DynamicImage::ImageRgba8(large_b)
        .save(&image_b)
        .expect("failed to create image_b");

    let open_started = Instant::now();
    let _img_a = image::open(&image_a).expect("failed to decode image_a");
    log_timing("open_image", open_started.elapsed(), OPEN_BUDGET);

    let nav_started = Instant::now();
    let _img_b = image::open(&image_b).expect("failed to decode image_b");
    log_timing("navigate_next", nav_started.elapsed(), NAVIGATION_BUDGET);

    if let Some(loaded_rss) = current_rss_bytes() {
        log_memory_budget("loaded_memory", loaded_rss, 260, 340);
    }
}
