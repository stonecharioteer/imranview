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
    let level = if elapsed_ms > budget.threshold_ms {
        "WARN"
    } else if elapsed_ms > budget.target_ms {
        "SLOW"
    } else {
        "OK"
    };

    eprintln!(
        "[perf][{level}] {label}={}ms (target={}ms threshold={}ms)",
        elapsed_ms, budget.target_ms, budget.threshold_ms
    );
}
