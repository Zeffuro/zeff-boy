use crate::debug::{
    ApuDebugInfo, ConsoleGraphicsData, CpuDebugSnapshot, DisassemblyView, InputDebugInfo,
    MemorySearchResult, OamDebugInfo, PaletteDebugInfo, PerfInfo, RomDebugInfo, RomSearchResult,
};
use crate::emu_backend::CoreCapabilities;
use zeff_emu_common::address::Address;

#[derive(Default)]
pub(crate) struct UiFrameData {
    pub(crate) core_features: Option<CoreCapabilities>,
    pub(crate) cpu_debug: Option<CpuDebugSnapshot>,
    pub(crate) perf_info: Option<PerfInfo>,
    pub(crate) apu_debug: Option<ApuDebugInfo>,
    pub(crate) oam_debug: Option<OamDebugInfo>,
    pub(crate) palette_debug: Option<PaletteDebugInfo>,
    pub(crate) rom_debug: Option<RomDebugInfo>,
    pub(crate) input_debug: Option<InputDebugInfo>,
    pub(crate) graphics_data: Option<ConsoleGraphicsData>,
    pub(crate) disassembly_view: Option<DisassemblyView>,
    pub(crate) memory_page: Option<Vec<(Address, u8)>>,
    pub(crate) memory_search_results: Option<Vec<MemorySearchResult>>,
    pub(crate) rom_page: Option<Vec<(u32, u8)>>,
    pub(crate) rom_size: u32,
    pub(crate) rom_search_results: Option<Vec<RomSearchResult>>,
}
