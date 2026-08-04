use zeff_gb_core::hardware::ppu::DmgPalettePreset;
use zeff_gb_core::hardware::types::hardware_mode::HardwareModePreference;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct HeadlessInputEvent {
    pub(crate) start_frame: u64,
    pub(crate) end_frame: u64,
    pub(crate) buttons: u8,
    pub(crate) dpad: u8,
    pub(crate) reset: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct HeadlessZapperEvent {
    pub(crate) start_frame: u64,
    pub(crate) end_frame: u64,
    pub(crate) x: u16,
    pub(crate) y: u16,
    pub(crate) trigger: bool,
    pub(crate) hit: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum HeadlessBusTraceAccess {
    Read,
    Write,
    ReadWrite,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct HeadlessBusTraceFilter {
    pub(crate) start_addr: u64,
    pub(crate) end_addr: u64,
    pub(crate) access: HeadlessBusTraceAccess,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct HeadlessMemoryDump {
    pub(crate) start_addr: u16,
    pub(crate) len: u16,
}

pub(crate) struct HeadlessOptions {
    pub(crate) max_frames: u64,
    pub(crate) expect_serial: Option<String>,
    pub(crate) expect_ws_text: Option<String>,
    pub(crate) expect_ws_pass_fail_tiles: bool,
    pub(crate) expect_test_pass: bool,
    pub(crate) trace_opcodes: bool,
    pub(crate) trace_opcode_limit: u64,
    pub(crate) trace_start_t: u64,
    pub(crate) trace_pc_range: Option<(u64, u64)>,
    pub(crate) trace_opcode_filter: Vec<u8>,
    pub(crate) trace_watch_interrupts: bool,
    pub(crate) trace_bus_filters: Vec<HeadlessBusTraceFilter>,
    pub(crate) trace_bus_limit: u64,
    pub(crate) memory_dumps: Vec<HeadlessMemoryDump>,
    pub(crate) break_at: Option<u16>,
    pub(crate) no_apu: bool,
    pub(crate) no_sram: bool,
    pub(crate) gb_dmg_palette_preset: Option<DmgPalettePreset>,
    pub(crate) stuck_window_frames: u64,
    pub(crate) stuck_pc_threshold: usize,
    pub(crate) fail_on_stuck: bool,
    pub(crate) input_events: Vec<HeadlessInputEvent>,
    pub(crate) zapper_events: Vec<HeadlessZapperEvent>,
    pub(crate) load_state_path: Option<std::path::PathBuf>,
    pub(crate) screenshot_path: Option<std::path::PathBuf>,
    pub(crate) screenshot_frame: Option<u64>,
    pub(crate) screenshot_dir: Option<std::path::PathBuf>,
    pub(crate) screenshot_every: u64,
    pub(crate) print_debug_state: bool,
    pub(crate) debug_state_path: Option<std::path::PathBuf>,
    pub(crate) audio_dump_path: Option<std::path::PathBuf>,
    pub(crate) break_on_gba_bad_state: bool,
    pub(crate) gba_audio_mutes: [bool; 6],
    pub(crate) gba_hidden_bg_layers: [bool; 4],
    pub(crate) gba_hide_sprites: bool,
    pub(crate) gba_dump_memory_dir: Option<std::path::PathBuf>,
}

impl Default for HeadlessOptions {
    fn default() -> Self {
        Self {
            max_frames: 600,
            expect_serial: None,
            expect_ws_text: None,
            expect_ws_pass_fail_tiles: false,
            expect_test_pass: false,
            trace_opcodes: false,
            trace_opcode_limit: 512,
            trace_start_t: 0,
            trace_pc_range: None,
            trace_opcode_filter: Vec::new(),
            trace_watch_interrupts: false,
            trace_bus_filters: Vec::new(),
            trace_bus_limit: 512,
            memory_dumps: Vec::new(),
            break_at: None,
            no_apu: false,
            no_sram: false,
            gb_dmg_palette_preset: None,
            stuck_window_frames: 0,
            stuck_pc_threshold: 8,
            fail_on_stuck: false,
            input_events: Vec::new(),
            zapper_events: Vec::new(),
            load_state_path: None,
            screenshot_path: None,
            screenshot_frame: None,
            screenshot_dir: None,
            screenshot_every: 0,
            print_debug_state: false,
            debug_state_path: None,
            audio_dump_path: None,
            break_on_gba_bad_state: false,
            gba_audio_mutes: [false; 6],
            gba_hidden_bg_layers: [false; 4],
            gba_hide_sprites: false,
            gba_dump_memory_dir: None,
        }
    }
}

pub(crate) struct CliArgs {
    pub(crate) rom_path: Option<String>,
    pub(crate) mode_override: Option<HardwareModePreference>,
    pub(crate) headless: Option<HeadlessOptions>,
}
