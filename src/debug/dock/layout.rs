use egui_dock::DockState;

use super::tabs::DebugTab;

pub(crate) fn create_default_dock_state() -> DockState<DebugTab> {
    let mut dock = DockState::new(vec![]);
    dock.add_window(vec![DebugTab::CpuDebug]);
    dock
}

pub(crate) fn create_ide_dock_state() -> DockState<DebugTab> {
    // Central area: Game view
    let mut dock = DockState::new(vec![DebugTab::GameView]);
    let tree = dock.main_surface_mut();

    // Left panel: CPU debug + Performance + APU + Input
    let [_center, _left] = tree.split_left(
        egui_dock::NodeIndex::root(),
        0.25,
        vec![
            DebugTab::CpuDebug,
            DebugTab::Performance,
            DebugTab::ApuViewer,
            DebugTab::InputViewer,
        ],
    );

    // Right panel: Disassembler + Memory + ROM Viewer
    let [_center2, right] = tree.split_right(
        egui_dock::NodeIndex::root(),
        0.65,
        vec![
            DebugTab::Disassembler,
            DebugTab::MemoryViewer,
            DebugTab::RomViewer,
        ],
    );

    // Bottom-right: Breakpoints + Cheats
    let [_right_top, _right_bottom] =
        tree.split_below(right, 0.65, vec![DebugTab::Breakpoints, DebugTab::Cheats]);

    // Below game view: Graphics viewers grouped together
    let [_center3, _bottom] = tree.split_below(
        egui_dock::NodeIndex::root(),
        0.6,
        vec![
            DebugTab::TileViewer,
            DebugTab::TilemapViewer,
            DebugTab::OamViewer,
            DebugTab::PaletteViewer,
        ],
    );

    dock
}

pub(crate) fn create_dock_from_saved_tabs(tab_names: &[String]) -> DockState<DebugTab> {
    let tabs: Vec<DebugTab> = tab_names
        .iter()
        .filter_map(|name| DebugTab::from_persist_name(name))
        .collect();
    if tabs.is_empty() {
        return create_default_dock_state();
    }

    let has_game_view = tabs.contains(&DebugTab::GameView);
    let non_game_tabs: Vec<DebugTab> = tabs
        .iter()
        .copied()
        .filter(|t| *t != DebugTab::GameView)
        .collect();

    if has_game_view {
        let mut dock = DockState::new(vec![DebugTab::GameView]);
        if !non_game_tabs.is_empty() {
            dock.add_window(non_game_tabs);
        }
        dock
    } else {
        let mut dock = DockState::new(vec![]);
        if !non_game_tabs.is_empty() {
            dock.add_window(non_game_tabs);
        }
        dock
    }
}

pub(crate) fn save_open_tabs(dock: &DockState<DebugTab>) -> Vec<String> {
    dock.iter_all_tabs()
        .map(|(_, tab)| tab.persist_name().to_string())
        .collect()
}

pub(crate) fn ensure_game_view_tab(dock: &mut DockState<DebugTab>) {
    if !is_tab_open(dock, DebugTab::GameView) {
        dock.main_surface_mut()
            .push_to_focused_leaf(DebugTab::GameView);
    }
}

pub(crate) fn toggle_dock_tab(dock: &mut DockState<DebugTab>, tab: DebugTab) {
    if let Some(loc) = dock.find_tab(&tab) {
        dock.remove_tab(loc);
    } else {
        dock.add_window(vec![tab]);
    }
}

pub(crate) fn is_tab_open(dock: &DockState<DebugTab>, tab: DebugTab) -> bool {
    dock.find_tab(&tab).is_some()
}

