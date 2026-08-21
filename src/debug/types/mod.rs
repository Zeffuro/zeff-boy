mod cheats;
mod data_models;
mod execution_coverage;
mod memory;
pub(crate) mod mods;
mod runtime_inference;
mod trace;
mod viewers;

pub(crate) use cheats::{BreakpointState, CheatState, LibretroAsyncResult};
pub(crate) use data_models::{
    ApuChannelDebug, ApuDebugInfo, CallStackDisplay, ConsoleGraphicsData, CpuDebugSnapshot,
    CpuDebugViewState, DebugSection, GbGraphicsData, GbaGraphicsData, InputDebugInfo, IoBitDisplay,
    IoRegisterDisplay, NesGraphicsData, OamDebugInfo, PaletteDebugInfo, PaletteGroupDebug,
    PaletteRowDebug, RecentOpcodeDisplay, RomDebugInfo, RomInfoSection, Sega8GraphicsData,
    WatchHitDisplay, WatchpointDisplay,
};
pub(crate) use execution_coverage::ExecutionCoverage;
pub(crate) use memory::{
    MemoryBookmark, MemoryByteDiff, MemorySearchMode, MemorySearchResult, MemoryViewerState,
    RomSearchResult, RomViewerState,
};
pub(crate) use mods::ModState;
pub(crate) use runtime_inference::RuntimeSymbolCandidate;
pub(crate) use trace::TraceViewerState;
pub(crate) use viewers::{
    OamViewerState, PerfInfo, SourceViewerState, TileViewerPlatform, TileViewerRequest,
    TileViewerState, TilemapViewerState,
};

use super::DisassemblyView;
use std::sync::Arc;
use zeff_emu_common::address::Address;

#[derive(Clone, Copy)]
pub(crate) struct DebugDataRefs<'a> {
    pub(crate) symbols: &'a crate::symbols::SymbolSession,
    pub(crate) cpu_debug: Option<&'a CpuDebugSnapshot>,
    pub(crate) perf_info: Option<&'a PerfInfo>,
    pub(crate) apu_debug: Option<&'a ApuDebugInfo>,
    pub(crate) oam_debug: Option<&'a OamDebugInfo>,
    pub(crate) palette_debug: Option<&'a PaletteDebugInfo>,
    pub(crate) rom_debug: Option<&'a RomDebugInfo>,
    pub(crate) input_debug: Option<&'a InputDebugInfo>,
    pub(crate) graphics_data: Option<&'a ConsoleGraphicsData>,
    pub(crate) disassembly_view: Option<&'a DisassemblyView>,
    pub(crate) memory_page: Option<&'a [(Address, u8)]>,
    pub(crate) rom_page: Option<&'a [(u32, u8)]>,
    pub(crate) rom_size: u32,
}

use crate::settings::{BindingAction, InputBindingAction, ShortcutAction, WonderSwanButton};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FirmwareInventoryStatusKind {
    Recognized,
    UnknownHash,
    WrongSize,
    NotFound,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct FirmwareInventoryRow {
    pub(crate) firmware_id: String,
    pub(crate) system: String,
    pub(crate) firmware: String,
    pub(crate) path: Option<String>,
    pub(crate) status: FirmwareInventoryStatusKind,
    pub(crate) detail: String,
    pub(crate) sha256_prefix: Option<String>,
    pub(crate) managed_key: Option<String>,
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) struct FirmwareInventoryScanResult {
    pub(crate) configured_directory: Option<std::path::PathBuf>,
    pub(crate) result: Result<
        (
            Vec<FirmwareInventoryRow>,
            Arc<zeff_firmware::FirmwareInventory>,
        ),
        String,
    >,
}

pub(crate) struct FirmwareInventoryState {
    pub(crate) directory: Option<std::path::PathBuf>,
    pub(crate) rows: Vec<FirmwareInventoryRow>,
    pub(crate) inventory: Option<Arc<zeff_firmware::FirmwareInventory>>,
    pub(crate) error: Option<String>,
    pub(crate) needs_refresh: bool,
    pub(crate) pending_removal: Option<String>,
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) scan_receiver: Option<std::sync::mpsc::Receiver<FirmwareInventoryScanResult>>,
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) import_receiver:
        Option<std::sync::mpsc::Receiver<Result<crate::platform::NativeFirmwareImport, String>>>,
    #[cfg(target_arch = "wasm32")]
    pub(crate) pending_file: crate::platform::FileDataSlot,
    #[cfg(target_arch = "wasm32")]
    pub(crate) web_operation_pending: bool,
    #[cfg(target_arch = "wasm32")]
    pub(crate) web_operation_result: std::rc::Rc<std::cell::RefCell<Option<Result<(), String>>>>,
}

impl Default for FirmwareInventoryState {
    fn default() -> Self {
        Self {
            directory: None,
            rows: Vec::new(),
            inventory: None,
            error: None,
            needs_refresh: true,
            pending_removal: None,
            #[cfg(not(target_arch = "wasm32"))]
            scan_receiver: None,
            #[cfg(not(target_arch = "wasm32"))]
            import_receiver: None,
            #[cfg(target_arch = "wasm32")]
            pending_file: std::rc::Rc::new(std::cell::RefCell::new(None)),
            #[cfg(target_arch = "wasm32")]
            web_operation_pending: false,
            #[cfg(target_arch = "wasm32")]
            web_operation_result: std::rc::Rc::new(std::cell::RefCell::new(None)),
        }
    }
}

pub(crate) struct DebugWindowState {
    pub(crate) cpu_view: CpuDebugViewState,
    pub(crate) hardware_view: CpuDebugViewState,
    pub(crate) memory: MemoryViewerState,
    pub(crate) bp: BreakpointState,
    pub(crate) rebinding_action: Option<InputBindingAction>,
    pub(crate) rebinding_shortcut: Option<ShortcutAction>,
    pub(crate) rebinding_gamepad: Option<BindingAction>,
    pub(crate) rebinding_gamepad_p2: Option<BindingAction>,
    pub(crate) rebinding_ws_gamepad: Option<WonderSwanButton>,
    pub(crate) rebinding_gamepad_action: Option<crate::settings::GamepadAction>,
    pub(crate) rebinding_speedup: bool,
    pub(crate) rebinding_rewind: bool,
    pub(crate) last_disasm_pc: Option<Address>,
    pub(crate) last_disasm_mapping: Option<u64>,
    pub(crate) disasm_target: Option<super::DisassemblyTarget>,
    pub(crate) disasm_back: Vec<Option<super::DisassemblyTarget>>,
    pub(crate) disasm_forward: Vec<Option<super::DisassemblyTarget>>,
    pub(crate) tilemap: TilemapViewerState,
    pub(crate) tiles: TileViewerState,
    pub(crate) oam: OamViewerState,
    pub(crate) rom_viewer: RomViewerState,
    pub(crate) source_viewer: SourceViewerState,
    pub(crate) symbol_browser: SymbolBrowserState,
    pub(crate) console: super::DebugConsoleState,
    pub(crate) perf_history: crate::debug::perf_monitor::PerfHistory,
    pub(crate) printer: crate::debug::PrinterViewerState,
    pub(crate) barcode_boy_scan_open: bool,
    pub(crate) barcode_boy_digits: String,
    pub(crate) trace: TraceViewerState,
    pub(crate) execution_coverage: ExecutionCoverage,
    pub(crate) settings_tab: usize,
    pub(crate) firmware_inventory: FirmwareInventoryState,
    pub(crate) camera_devices: Vec<crate::camera::CameraDeviceInfo>,
    pub(crate) camera_device_error: Option<String>,
    pub(crate) camera_devices_needs_refresh: bool,
    pub(crate) cheat: CheatState,
    pub(crate) mod_state: ModState,
    pub(crate) layer_enable_bg: bool,
    pub(crate) gba_layer_enable_bg: [bool; 4],
    pub(crate) layer_enable_window: bool,
    pub(crate) layer_enable_sprites: bool,
    pub(crate) tile_viewer_was_open: bool,
    pub(crate) tilemap_viewer_was_open: bool,
}

impl DebugWindowState {
    pub(crate) fn new() -> Self {
        Self {
            cpu_view: CpuDebugViewState::default(),
            hardware_view: CpuDebugViewState::default(),
            memory: MemoryViewerState::new(),
            bp: BreakpointState::new(),
            rebinding_action: None,
            rebinding_shortcut: None,
            rebinding_gamepad: None,
            rebinding_gamepad_p2: None,
            rebinding_ws_gamepad: None,
            rebinding_gamepad_action: None,
            rebinding_speedup: false,
            rebinding_rewind: false,
            last_disasm_pc: None,
            last_disasm_mapping: None,
            disasm_target: None,
            disasm_back: Vec::new(),
            disasm_forward: Vec::new(),
            tilemap: TilemapViewerState::new(),
            tiles: TileViewerState::new(),
            oam: OamViewerState::new(),
            rom_viewer: RomViewerState::new(),
            source_viewer: SourceViewerState::default(),
            symbol_browser: SymbolBrowserState::default(),
            console: super::DebugConsoleState::default(),
            perf_history: crate::debug::perf_monitor::PerfHistory::new(),
            printer: crate::debug::PrinterViewerState::default(),
            barcode_boy_scan_open: false,
            barcode_boy_digits: String::new(),
            trace: TraceViewerState::new(),
            execution_coverage: ExecutionCoverage::default(),
            settings_tab: 0,
            firmware_inventory: FirmwareInventoryState::default(),
            camera_devices: Vec::new(),
            camera_device_error: None,
            camera_devices_needs_refresh: true,
            cheat: CheatState::new(),
            mod_state: ModState::new(),
            layer_enable_bg: true,
            gba_layer_enable_bg: [true; 4],
            layer_enable_window: true,
            layer_enable_sprites: true,
            tile_viewer_was_open: false,
            tilemap_viewer_was_open: false,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum SymbolLocationFilter {
    #[default]
    All,
    Rom,
    CpuOnly,
    Constants,
}

impl SymbolLocationFilter {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::All => "All locations",
            Self::Rom => "ROM",
            Self::CpuOnly => "CPU only",
            Self::Constants => "Constants",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum SymbolExecutionFilter {
    #[default]
    All,
    Executed,
    Unexecuted,
}

impl SymbolExecutionFilter {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::All => "Any execution",
            Self::Executed => "Executed",
            Self::Unexecuted => "Not executed",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum SymbolSort {
    #[default]
    Search,
    MostExecuted,
}

impl SymbolSort {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Search => "Search order",
            Self::MostExecuted => "Most executed",
        }
    }
}

#[derive(Default)]
pub(crate) struct SymbolBrowserState {
    pub(crate) query: String,
    pub(crate) last_query: String,
    pub(crate) last_generation: u64,
    pub(crate) kind_filter: Option<crate::symbols::SymbolKind>,
    pub(crate) last_kind_filter: Option<crate::symbols::SymbolKind>,
    pub(crate) location_filter: SymbolLocationFilter,
    pub(crate) last_location_filter: SymbolLocationFilter,
    pub(crate) execution_filter: SymbolExecutionFilter,
    pub(crate) last_execution_filter: SymbolExecutionFilter,
    pub(crate) sort: SymbolSort,
    pub(crate) last_sort: SymbolSort,
    pub(crate) last_coverage_revision: u64,
    pub(crate) results: Vec<crate::symbols::SymbolId>,
    pub(crate) selected: Option<crate::symbols::SymbolId>,
    pub(crate) editor: Option<SymbolEditorState>,
}

pub(crate) struct SymbolEditorState {
    pub(crate) original_user_name: Option<String>,
    pub(crate) draft: crate::symbols::UserSymbolDraft,
}

fn fold_bytes(bytes: &[u8]) -> u64 {
    crc32fast::hash(bytes) as u64
}
