use egui_dock::DockState;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum DebugTab {
    GameView,
    CpuDebug,
    InputViewer,
    ApuViewer,
    RomInfo,
    Disassembler,
    MemoryViewer,
    TileViewer,
    TilemapViewer,
    OamViewer,
    PaletteViewer,
    Performance,
    Breakpoints,
    Cheats,
    RomViewer,
    Mods,
}

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct TabDataRequirements {
    pub(crate) needs_debug_info: bool,
    pub(crate) needs_perf_info: bool,
    pub(crate) needs_viewer_data: bool,
    pub(crate) needs_vram: bool,
    pub(crate) needs_oam: bool,
    pub(crate) needs_apu: bool,
    pub(crate) needs_disassembly: bool,
    pub(crate) needs_rom_info: bool,
    pub(crate) needs_memory_page: bool,
    pub(crate) needs_rom_page: bool,
}

impl DebugTab {
    pub(crate) fn requirements(self) -> TabDataRequirements {
        match self {
            DebugTab::GameView => TabDataRequirements::default(),
            DebugTab::CpuDebug => TabDataRequirements {
                needs_debug_info: true,
                ..Default::default()
            },
            DebugTab::InputViewer => TabDataRequirements {
                needs_debug_info: true,
                ..Default::default()
            },
            DebugTab::Performance => TabDataRequirements {
                needs_perf_info: true,
                ..Default::default()
            },
            DebugTab::Breakpoints => TabDataRequirements {
                needs_debug_info: true,
                ..Default::default()
            },
            DebugTab::ApuViewer => TabDataRequirements {
                needs_viewer_data: true,
                needs_apu: true,
                ..Default::default()
            },
            DebugTab::TileViewer => TabDataRequirements {
                needs_viewer_data: true,
                needs_vram: true,
                ..Default::default()
            },
            DebugTab::TilemapViewer => TabDataRequirements {
                needs_viewer_data: true,
                needs_vram: true,
                ..Default::default()
            },
            DebugTab::OamViewer => TabDataRequirements {
                needs_viewer_data: true,
                needs_oam: true,
                ..Default::default()
            },
            DebugTab::PaletteViewer => TabDataRequirements {
                needs_viewer_data: true,
                ..Default::default()
            },
            DebugTab::RomInfo => TabDataRequirements {
                needs_rom_info: true,
                ..Default::default()
            },
            DebugTab::Disassembler => TabDataRequirements {
                needs_disassembly: true,
                ..Default::default()
            },
            DebugTab::MemoryViewer => TabDataRequirements {
                needs_memory_page: true,
                ..Default::default()
            },
            DebugTab::Cheats => TabDataRequirements::default(),
            DebugTab::RomViewer => TabDataRequirements {
                needs_rom_page: true,
                ..Default::default()
            },
            DebugTab::Mods => TabDataRequirements::default(),
        }
    }
}

pub(crate) fn compute_tab_requirements(dock: &DockState<DebugTab>) -> TabDataRequirements {
    let mut reqs = TabDataRequirements::default();
    for (_, tab) in dock.iter_all_tabs() {
        let r = tab.requirements();
        reqs.needs_debug_info |= r.needs_debug_info;
        reqs.needs_perf_info |= r.needs_perf_info;
        reqs.needs_viewer_data |= r.needs_viewer_data;
        reqs.needs_vram |= r.needs_vram;
        reqs.needs_oam |= r.needs_oam;
        reqs.needs_apu |= r.needs_apu;
        reqs.needs_disassembly |= r.needs_disassembly;
        reqs.needs_rom_info |= r.needs_rom_info;
        reqs.needs_memory_page |= r.needs_memory_page;
        reqs.needs_rom_page |= r.needs_rom_page;
    }
    reqs
}

const TAB_META: &[(DebugTab, &str, &str)] = &[
    (DebugTab::GameView, "Game", "GameView"),
    (DebugTab::CpuDebug, "CPU / Debug", "CpuDebug"),
    (DebugTab::InputViewer, "Input", "InputViewer"),
    (DebugTab::ApuViewer, "APU / Sound", "ApuViewer"),
    (DebugTab::RomInfo, "ROM Info", "RomInfo"),
    (DebugTab::Disassembler, "Disassembler", "Disassembler"),
    (DebugTab::MemoryViewer, "Memory Viewer", "MemoryViewer"),
    (DebugTab::RomViewer, "ROM Viewer", "RomViewer"),
    (DebugTab::TileViewer, "Tile Data", "TileViewer"),
    (DebugTab::TilemapViewer, "Tile Map", "TilemapViewer"),
    (DebugTab::OamViewer, "OAM / Sprites", "OamViewer"),
    (DebugTab::PaletteViewer, "Palettes", "PaletteViewer"),
    (DebugTab::Performance, "Performance", "Performance"),
    (DebugTab::Breakpoints, "Breakpoints", "Breakpoints"),
    (DebugTab::Cheats, "Cheats", "Cheats"),
    (DebugTab::Mods, "Mods", "Mods"),
];

impl DebugTab {
    pub(super) fn title(self) -> &'static str {
        TAB_META
            .iter()
            .find(|(t, _, _)| *t == self)
            .map(|(_, title, _)| *title)
            .unwrap_or("?")
    }

    pub(crate) fn persist_name(self) -> &'static str {
        TAB_META
            .iter()
            .find(|(t, _, _)| *t == self)
            .map(|(_, _, name)| *name)
            .unwrap_or("?")
    }

    pub(crate) fn from_persist_name(name: &str) -> Option<Self> {
        TAB_META
            .iter()
            .find(|(_, _, n)| *n == name)
            .map(|(tab, _, _)| *tab)
    }
}
