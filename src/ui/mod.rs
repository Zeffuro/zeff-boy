use crate::debug::{
    ApuDebugInfo, ConsoleGraphicsData, CpuDebugSnapshot, DebugUiActions, DisassemblyView,
    InputDebugInfo, MemorySearchResult, OamDebugInfo, PaletteDebugInfo, PerfInfo, RomDebugInfo,
    RomSearchResult,
};

mod gb_snapshot;
mod nes_snapshot;

pub(crate) use gb_snapshot::collect_emu_snapshot;
pub(crate) use nes_snapshot::collect_nes_snapshot;

pub(crate) struct UiFrameData {
    pub(crate) cpu_debug: Option<CpuDebugSnapshot>,
    pub(crate) perf_info: Option<PerfInfo>,
    pub(crate) apu_debug: Option<ApuDebugInfo>,
    pub(crate) oam_debug: Option<OamDebugInfo>,
    pub(crate) palette_debug: Option<PaletteDebugInfo>,
    pub(crate) rom_debug: Option<RomDebugInfo>,
    pub(crate) input_debug: Option<InputDebugInfo>,
    pub(crate) graphics_data: Option<ConsoleGraphicsData>,
    pub(crate) disassembly_view: Option<DisassemblyView>,
    pub(crate) memory_page: Option<Vec<(u16, u8)>>,
    pub(crate) memory_search_results: Option<Vec<MemorySearchResult>>,
    pub(crate) rom_page: Option<Vec<(u32, u8)>>,
    pub(crate) rom_size: u32,
    pub(crate) rom_search_results: Option<Vec<RomSearchResult>>,
}

pub(crate) fn empty_frame_data() -> UiFrameData {
    UiFrameData {
        cpu_debug: None,
        perf_info: None,
        apu_debug: None,
        oam_debug: None,
        palette_debug: None,
        rom_debug: None,
        input_debug: None,
        graphics_data: None,
        disassembly_view: None,
        memory_page: None,
        memory_search_results: None,
        rom_page: None,
        rom_size: 0,
        rom_search_results: None,
    }
}

pub(crate) fn apply_debug_actions(
    actions: &DebugUiActions,
    debug_step_requested: &mut bool,
    debug_continue_requested: &mut bool,
    backstep_requested: &mut bool,
) {
    if actions.step_requested {
        *debug_step_requested = true;
    }
    if actions.continue_requested {
        *debug_continue_requested = true;
    }
    if actions.backstep_requested {
        *backstep_requested = true;
    }
}
