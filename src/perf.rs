use std::time::Duration;

pub struct PerfBudget {
    pub target_ms: u128,
    pub threshold_ms: u128,
}

pub const STARTUP_BUDGET: PerfBudget = PerfBudget {
    target_ms: 450,
    threshold_ms: 700,
};
pub const OPEN_IMAGE_BUDGET: PerfBudget = PerfBudget {
    target_ms: 150,
    threshold_ms: 300,
};
pub const OPEN_QUEUE_BUDGET: PerfBudget = PerfBudget {
    target_ms: 40,
    threshold_ms: 120,
};
pub const OPEN_PICKER_BUDGET: PerfBudget = PerfBudget {
    target_ms: 200,
    threshold_ms: 750,
};
#[cfg(test)]
pub const NAVIGATION_BUDGET: PerfBudget = PerfBudget {
    target_ms: 90,
    threshold_ms: 180,
};
pub const SAVE_IMAGE_BUDGET: PerfBudget = PerfBudget {
    target_ms: 250,
    threshold_ms: 600,
};
pub const EDIT_IMAGE_BUDGET: PerfBudget = PerfBudget {
    target_ms: 120,
    threshold_ms: 300,
};

pub fn log_timing(label: &str, elapsed: Duration, budget: PerfBudget) {
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
