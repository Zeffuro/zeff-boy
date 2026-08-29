use zeff_gb_core::hardware::ppu::DmgPalettePreset;
use zeff_gb_core::hardware::types::hardware_mode::HardwareModePreference;
use zeff_pce_core::hardware::{PceArcadeCardMode, PceControllerMode, PceMemoryBaseMode};
use zeff_sega8_core::hardware::region::Sega8Region;
use zeff_sega8_core::hardware::timing::Sega8VideoStandard;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct HeadlessInputEvent {
    pub(crate) start_frame: u64,
    pub(crate) end_frame: u64,
    pub(crate) buttons: u8,
    pub(crate) dpad: u8,
    pub(crate) coleco_keypad: Option<u8>,
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct HeadlessRegionDump {
    pub(crate) region: String,
    pub(crate) offset: usize,
    pub(crate) len: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct HeadlessTasAssertion {
    pub(crate) name: String,
    pub(crate) frame: u64,
    pub(crate) pc: Option<u32>,
    pub(crate) state_sha256: Option<[u8; 32]>,
    pub(crate) framebuffer_sha256: Option<[u8; 32]>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct HeadlessTasScript {
    pub(crate) system: String,
    pub(crate) assertions: Vec<HeadlessTasAssertion>,
}

pub(crate) struct HeadlessOptions {
    pub(crate) tas_project_path: Option<std::path::PathBuf>,
    pub(crate) tas_branch_id: Option<String>,
    pub(crate) tas_export_path: Option<std::path::PathBuf>,
    pub(crate) max_frames: u64,
    pub(crate) expect_serial: Option<String>,
    pub(crate) expect_ws_text: Option<String>,
    pub(crate) expect_ws_pass_fail_tiles: bool,
    pub(crate) ws_link_peer_path: Option<std::path::PathBuf>,
    pub(crate) expect_ws_link_bytes: u64,
    pub(crate) expect_sega8_sdsc: Option<String>,
    pub(crate) expect_sega8_audio: bool,
    pub(crate) expect_coleco_audio: bool,
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
    pub(crate) region_dumps: Vec<HeadlessRegionDump>,
    pub(crate) break_at: Option<u16>,
    pub(crate) no_apu: bool,
    pub(crate) no_sram: bool,
    pub(crate) apply_mods: bool,
    pub(crate) gb_dmg_palette_preset: Option<DmgPalettePreset>,
    pub(crate) stuck_window_frames: u64,
    pub(crate) stuck_pc_threshold: usize,
    pub(crate) fail_on_stuck: bool,
    pub(crate) input_events: Vec<HeadlessInputEvent>,
    pub(crate) input_events_p2: Vec<HeadlessInputEvent>,
    pub(crate) input_events_p3: Vec<HeadlessInputEvent>,
    pub(crate) input_events_p4: Vec<HeadlessInputEvent>,
    pub(crate) input_events_p5: Vec<HeadlessInputEvent>,
    pub(crate) tas_script: Option<HeadlessTasScript>,
    pub(crate) zapper_events: Vec<HeadlessZapperEvent>,
    pub(crate) load_state_path: Option<std::path::PathBuf>,
    pub(crate) pce_save_state_path: Option<std::path::PathBuf>,
    pub(crate) coleco_save_state_path: Option<std::path::PathBuf>,
    pub(crate) replay_path: Option<std::path::PathBuf>,
    pub(crate) replay_peer_path: Option<std::path::PathBuf>,
    pub(crate) replay_peer_live_link: bool,
    pub(crate) replay_tail_frames: u64,
    pub(crate) expect_gb_link_events: u64,
    pub(crate) allow_gb_link_replay_divergence: bool,
    pub(crate) expect_replay_final_hash: Option<String>,
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
    pub(crate) sega8_video_standard: Option<Sega8VideoStandard>,
    pub(crate) sega8_console_region: Option<Sega8Region>,
    pub(crate) pce_controller_mode: Option<PceControllerMode>,
    pub(crate) pce_memory_base_mode: Option<PceMemoryBaseMode>,
    pub(crate) pce_arcade_card_mode: Option<PceArcadeCardMode>,
}

impl Default for HeadlessOptions {
    fn default() -> Self {
        Self {
            tas_project_path: None,
            tas_branch_id: None,
            tas_export_path: None,
            max_frames: 600,
            expect_serial: None,
            expect_ws_text: None,
            expect_ws_pass_fail_tiles: false,
            ws_link_peer_path: None,
            expect_ws_link_bytes: 0,
            expect_sega8_sdsc: None,
            expect_sega8_audio: false,
            expect_coleco_audio: false,
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
            region_dumps: Vec::new(),
            break_at: None,
            no_apu: false,
            no_sram: false,
            apply_mods: false,
            gb_dmg_palette_preset: None,
            stuck_window_frames: 0,
            stuck_pc_threshold: 8,
            fail_on_stuck: false,
            input_events: Vec::new(),
            input_events_p2: Vec::new(),
            input_events_p3: Vec::new(),
            input_events_p4: Vec::new(),
            input_events_p5: Vec::new(),
            tas_script: None,
            zapper_events: Vec::new(),
            load_state_path: None,
            pce_save_state_path: None,
            coleco_save_state_path: None,
            replay_path: None,
            replay_peer_path: None,
            replay_peer_live_link: false,
            replay_tail_frames: 0,
            expect_gb_link_events: 0,
            allow_gb_link_replay_divergence: false,
            expect_replay_final_hash: None,
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
            sega8_video_standard: None,
            sega8_console_region: None,
            pce_controller_mode: None,
            pce_memory_base_mode: None,
            pce_arcade_card_mode: None,
        }
    }
}

pub(crate) struct CliArgs {
    pub(crate) rom_path: Option<String>,
    pub(crate) mode_override: Option<HardwareModePreference>,
    pub(crate) headless: Option<HeadlessOptions>,
}
