use zeff_gb_core::hardware::types::hardware_mode::HardwareModePreference;

pub(crate) struct HeadlessOptions {
    pub(crate) max_frames: u64,
    pub(crate) expect_serial: Option<String>,
    pub(crate) trace_opcodes: bool,
    pub(crate) trace_opcode_limit: u64,
    pub(crate) trace_start_t: u64,
    pub(crate) trace_pc_range: Option<(u16, u16)>,
    pub(crate) trace_opcode_filter: Vec<u8>,
    pub(crate) trace_watch_interrupts: bool,
    pub(crate) break_at: Option<u16>,
}

impl Default for HeadlessOptions {
    fn default() -> Self {
        Self {
            max_frames: 600,
            expect_serial: None,
            trace_opcodes: false,
            trace_opcode_limit: 512,
            trace_start_t: 0,
            trace_pc_range: None,
            trace_opcode_filter: Vec::new(),
            trace_watch_interrupts: false,
            break_at: None,
        }
    }
}

pub(crate) struct CliArgs {
    pub(crate) rom_path: Option<String>,
    pub(crate) mode_override: Option<HardwareModePreference>,
    pub(crate) headless: Option<HeadlessOptions>,
}
