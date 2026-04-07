#[derive(Default)]
pub(crate) struct PendingRequests {
    pub(crate) latest_open: u64,
    pub(crate) latest_save: u64,
    pub(crate) latest_edit: u64,
    pub(crate) latest_batch: u64,
    pub(crate) latest_file: u64,
    pub(crate) latest_compare: u64,
    pub(crate) latest_print: u64,
    pub(crate) latest_utility: u64,
    pub(crate) open_inflight: bool,
    pub(crate) save_inflight: bool,
    pub(crate) edit_inflight: bool,
    pub(crate) batch_inflight: bool,
    pub(crate) file_inflight: bool,
    pub(crate) compare_inflight: bool,
    pub(crate) print_inflight: bool,
    pub(crate) utility_inflight: bool,
    pub(crate) picker_inflight: bool,
    pub(crate) queued_navigation_steps: i32,
}

impl PendingRequests {
    pub(crate) fn has_inflight(&self) -> bool {
        self.open_inflight
            || self.save_inflight
            || self.edit_inflight
            || self.batch_inflight
            || self.file_inflight
            || self.compare_inflight
            || self.print_inflight
            || self.utility_inflight
            || self.picker_inflight
    }
}
