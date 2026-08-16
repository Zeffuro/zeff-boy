use egui_dock::DockState;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
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
    SymbolBrowser,
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

impl TabDataRequirements {
    fn include(&mut self, other: Self) {
        self.needs_debug_info |= other.needs_debug_info;
        self.needs_perf_info |= other.needs_perf_info;
        self.needs_viewer_data |= other.needs_viewer_data;
        self.needs_vram |= other.needs_vram;
        self.needs_oam |= other.needs_oam;
        self.needs_apu |= other.needs_apu;
        self.needs_disassembly |= other.needs_disassembly;
        self.needs_rom_info |= other.needs_rom_info;
        self.needs_memory_page |= other.needs_memory_page;
        self.needs_rom_page |= other.needs_rom_page;
    }
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
            DebugTab::SymbolBrowser => TabDataRequirements::default(),
        }
    }
}

pub(crate) fn compute_tab_requirements(dock: &DockState<DebugTab>) -> TabDataRequirements {
    let mut reqs = TabDataRequirements::default();
    for (_, leaf) in dock.iter_leaves() {
        if leaf.collapsed {
            continue;
        }
        if let Some(tab) = leaf.tabs.get(leaf.active.0) {
            reqs.include(tab.requirements());
        }
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
    (DebugTab::SymbolBrowser, "Symbols", "SymbolBrowser"),
];

impl DebugTab {
    pub(crate) fn title(self) -> &'static str {
        TAB_META
            .iter()
            .find(|(t, _, _)| *t == self)
            .map(|(_, title, _)| *title)
            .unwrap_or("?")
    }

    pub(crate) fn all_tools() -> impl Iterator<Item = Self> {
        TAB_META
            .iter()
            .map(|(tab, _, _)| *tab)
            .filter(|tab| *tab != Self::GameView)
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

#[cfg(test)]
mod tests {
    use super::*;
    use egui_dock::{NodeIndex, SurfaceIndex, TabIndex, TabPath};

    #[test]
    fn requirements_ignore_inactive_tabs_in_same_stack() {
        let dock = DockState::new(vec![
            DebugTab::GameView,
            DebugTab::CpuDebug,
            DebugTab::ApuViewer,
        ]);

        let reqs = compute_tab_requirements(&dock);

        assert!(!reqs.needs_debug_info);
        assert!(!reqs.needs_apu);
        assert!(!reqs.needs_viewer_data);
    }

    #[test]
    fn requirements_follow_active_tab_in_stack() {
        let mut dock = DockState::new(vec![
            DebugTab::GameView,
            DebugTab::CpuDebug,
            DebugTab::ApuViewer,
        ]);
        dock.set_active_tab(TabPath::new(
            SurfaceIndex::main(),
            NodeIndex::root(),
            TabIndex(1),
        ))
        .unwrap();

        let reqs = compute_tab_requirements(&dock);

        assert!(reqs.needs_debug_info);
        assert!(!reqs.needs_apu);
        assert!(!reqs.needs_viewer_data);
    }

    #[test]
    fn requirements_include_active_tabs_from_multiple_leaves() {
        let mut dock = DockState::new(vec![DebugTab::GameView]);
        dock.main_surface_mut()
            .split_left(NodeIndex::root(), 0.5, vec![DebugTab::CpuDebug]);

        let reqs = compute_tab_requirements(&dock);

        assert!(reqs.needs_debug_info);
    }
}
