use egui_dock::{DockState, NodeIndex};

use super::DebugTab;
use crate::settings::DebugPresentation;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DebugWorkspacePreset {
    Balanced,
    Cpu,
    Graphics,
    Memory,
}

impl DebugWorkspacePreset {
    pub(crate) const ALL: [Self; 4] = [Self::Balanced, Self::Cpu, Self::Graphics, Self::Memory];

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Balanced => "Balanced",
            Self::Cpu => "CPU Debugging",
            Self::Graphics => "Graphics Debugging",
            Self::Memory => "Memory Debugging",
        }
    }
}

pub(crate) fn create_workspace_dock_state(
    presentation: DebugPresentation,
    preset: DebugWorkspacePreset,
) -> DockState<DebugTab> {
    match (presentation, preset) {
        (DebugPresentation::Ide, DebugWorkspacePreset::Balanced) => {
            super::layout::create_ide_dock_state()
        }
        (DebugPresentation::Ide, DebugWorkspacePreset::Cpu) => create_ide_cpu(),
        (DebugPresentation::Ide, DebugWorkspacePreset::Graphics) => create_ide_graphics(),
        (DebugPresentation::Ide, DebugWorkspacePreset::Memory) => create_ide_memory(),
        (DebugPresentation::Floating, DebugWorkspacePreset::Balanced) => {
            super::layout::create_default_dock_state()
        }
        (DebugPresentation::Floating, _) => create_debugger(preset),
        (_, DebugWorkspacePreset::Balanced) => super::layout::create_debugger_dock_state(),
        (_, _) => create_debugger(preset),
    }
}

fn create_debugger(preset: DebugWorkspacePreset) -> DockState<DebugTab> {
    match preset {
        DebugWorkspacePreset::Balanced => super::layout::create_debugger_dock_state(),
        DebugWorkspacePreset::Cpu => {
            let mut dock = DockState::new(vec![DebugTab::Disassembler, DebugTab::SourceViewer]);
            let tree = dock.main_surface_mut();
            let [center, _cpu] = tree.split_left(
                NodeIndex::root(),
                0.2,
                vec![
                    DebugTab::CpuDebug,
                    DebugTab::HardwareIo,
                    DebugTab::Breakpoints,
                ],
            );
            let [top, _history] = tree.split_below(
                center,
                0.7,
                vec![DebugTab::ExecutionHistory, DebugTab::CallStack],
            );
            tree.split_right(
                top,
                0.68,
                vec![DebugTab::MemoryViewer, DebugTab::SymbolBrowser],
            );
            dock
        }
        DebugWorkspacePreset::Graphics => {
            let mut dock = DockState::new(vec![DebugTab::TilemapViewer]);
            let tree = dock.main_surface_mut();
            let [center, _cpu] = tree.split_left(
                NodeIndex::root(),
                0.18,
                vec![
                    DebugTab::CpuDebug,
                    DebugTab::HardwareIo,
                    DebugTab::Performance,
                ],
            );
            let [top, _tiles] = tree.split_below(
                center,
                0.62,
                vec![DebugTab::TileViewer, DebugTab::PaletteViewer],
            );
            tree.split_right(top, 0.62, vec![DebugTab::OamViewer]);
            dock
        }
        DebugWorkspacePreset::Memory => {
            let mut dock = DockState::new(vec![DebugTab::MemoryViewer]);
            let tree = dock.main_surface_mut();
            let [center, _cpu] = tree.split_left(
                NodeIndex::root(),
                0.18,
                vec![
                    DebugTab::CpuDebug,
                    DebugTab::HardwareIo,
                    DebugTab::Breakpoints,
                ],
            );
            let [top, _history] = tree.split_below(
                center,
                0.72,
                vec![DebugTab::ExecutionHistory, DebugTab::CallStack],
            );
            tree.split_right(
                top,
                0.64,
                vec![
                    DebugTab::Disassembler,
                    DebugTab::RomViewer,
                    DebugTab::SymbolBrowser,
                ],
            );
            dock
        }
    }
}

fn create_ide_cpu() -> DockState<DebugTab> {
    let mut dock = DockState::new(vec![DebugTab::GameView]);
    let tree = dock.main_surface_mut();
    let [game, _cpu] = tree.split_left(
        NodeIndex::root(),
        0.18,
        vec![
            DebugTab::CpuDebug,
            DebugTab::HardwareIo,
            DebugTab::Breakpoints,
        ],
    );
    let [_game, right] = tree.split_right(
        game,
        0.56,
        vec![DebugTab::Disassembler, DebugTab::SourceViewer],
    );
    let [right_top, _history] = tree.split_below(
        right,
        0.7,
        vec![DebugTab::ExecutionHistory, DebugTab::CallStack],
    );
    tree.split_right(right_top, 0.68, vec![DebugTab::MemoryViewer]);
    dock
}

fn create_ide_graphics() -> DockState<DebugTab> {
    let mut dock = DockState::new(vec![DebugTab::GameView]);
    let tree = dock.main_surface_mut();
    let [game, _cpu] = tree.split_left(
        NodeIndex::root(),
        0.16,
        vec![
            DebugTab::CpuDebug,
            DebugTab::HardwareIo,
            DebugTab::Performance,
        ],
    );
    let [_game, map] = tree.split_right(game, 0.56, vec![DebugTab::TilemapViewer]);
    tree.split_below(
        game,
        0.68,
        vec![DebugTab::OamViewer, DebugTab::PaletteViewer],
    );
    tree.split_below(map, 0.62, vec![DebugTab::TileViewer]);
    dock
}

fn create_ide_memory() -> DockState<DebugTab> {
    let mut dock = DockState::new(vec![DebugTab::GameView]);
    let tree = dock.main_surface_mut();
    let [game, _cpu] = tree.split_left(
        NodeIndex::root(),
        0.17,
        vec![
            DebugTab::CpuDebug,
            DebugTab::HardwareIo,
            DebugTab::Breakpoints,
        ],
    );
    let [_game, right] = tree.split_right(game, 0.52, vec![DebugTab::MemoryViewer]);
    let [right_top, _history] = tree.split_below(
        right,
        0.72,
        vec![DebugTab::ExecutionHistory, DebugTab::CallStack],
    );
    tree.split_right(
        right_top,
        0.62,
        vec![
            DebugTab::Disassembler,
            DebugTab::RomViewer,
            DebugTab::SymbolBrowser,
        ],
    );
    dock
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::debug::{compute_tab_requirements, is_tab_open};
    use std::collections::HashSet;

    #[test]
    fn ide_presets_keep_game_and_multiple_live_tools() {
        for preset in DebugWorkspacePreset::ALL {
            let dock = create_workspace_dock_state(DebugPresentation::Ide, preset);
            assert!(is_tab_open(&dock, DebugTab::GameView));
            assert!(compute_tab_requirements(&dock).needs_debug_info);
        }
    }

    #[test]
    fn external_presets_do_not_add_game_view() {
        for preset in DebugWorkspacePreset::ALL {
            let dock = create_workspace_dock_state(DebugPresentation::GameAndDebugger, preset);
            assert!(!is_tab_open(&dock, DebugTab::GameView));
        }
    }

    #[test]
    fn presets_do_not_duplicate_tools() {
        for presentation in [
            DebugPresentation::GameAndDebugger,
            DebugPresentation::Floating,
            DebugPresentation::Ide,
        ] {
            for preset in DebugWorkspacePreset::ALL {
                let dock = create_workspace_dock_state(presentation, preset);
                let mut tabs = HashSet::new();
                for (_, tab) in dock.iter_all_tabs() {
                    assert!(tabs.insert(*tab), "duplicate {tab:?} in {presentation:?}");
                }
            }
        }
    }
}
