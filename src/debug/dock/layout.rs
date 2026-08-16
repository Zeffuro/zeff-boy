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
            DebugTab::SymbolBrowser,
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

pub(crate) fn create_debugger_dock_state() -> DockState<DebugTab> {
    let mut dock = DockState::new(vec![
        DebugTab::CpuDebug,
        DebugTab::Performance,
        DebugTab::ApuViewer,
        DebugTab::InputViewer,
    ]);
    let tree = dock.main_surface_mut();

    let [_left, right] = tree.split_right(
        egui_dock::NodeIndex::root(),
        0.48,
        vec![
            DebugTab::Disassembler,
            DebugTab::MemoryViewer,
            DebugTab::RomViewer,
            DebugTab::RomInfo,
            DebugTab::SymbolBrowser,
        ],
    );
    let [_right_top, _right_bottom] =
        tree.split_below(right, 0.68, vec![DebugTab::Breakpoints, DebugTab::Cheats]);
    let [_top, _bottom] = tree.split_below(
        egui_dock::NodeIndex::root(),
        0.7,
        vec![
            DebugTab::TileViewer,
            DebugTab::TilemapViewer,
            DebugTab::OamViewer,
            DebugTab::PaletteViewer,
        ],
    );

    dock
}

pub(crate) fn create_dock_for_presentation(
    presentation: crate::settings::DebugPresentation,
) -> DockState<DebugTab> {
    match presentation {
        crate::settings::DebugPresentation::GameAndDebugger => create_debugger_dock_state(),
        crate::settings::DebugPresentation::Floating => create_default_dock_state(),
        crate::settings::DebugPresentation::Ide => create_ide_dock_state(),
    }
}

pub(crate) fn restore_dock_layout(
    presentation: crate::settings::DebugPresentation,
    saved: Option<&serde_json::Value>,
    legacy_tabs: &[String],
) -> DockState<DebugTab> {
    let mut dock = saved
        .and_then(|value| serde_json::from_value(value.clone()).ok())
        .or_else(|| (!legacy_tabs.is_empty()).then(|| create_dock_from_saved_tabs(legacy_tabs)))
        .unwrap_or_else(|| create_dock_for_presentation(presentation));
    if presentation == crate::settings::DebugPresentation::GameAndDebugger
        && let Some(location) = dock.find_tab(&DebugTab::GameView)
    {
        dock.remove_tab(location);
    }
    dock
}

pub(crate) fn serialize_dock_layout(dock: &DockState<DebugTab>) -> Option<serde_json::Value> {
    serde_json::to_value(dock).ok()
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

pub(crate) fn activate_dock_tab(dock: &mut DockState<DebugTab>, tab: DebugTab) {
    if let Some(path) = dock.find_tab(&tab) {
        let node_path = path.node_path();
        let _ = dock.set_active_tab(path);
        dock.set_focused_node_and_surface(node_path);
    } else {
        dock.push_to_focused_leaf(tab);
    }
}

pub(crate) fn is_tab_open(dock: &DockState<DebugTab>, tab: DebugTab) -> bool {
    dock.find_tab(&tab).is_some()
}

#[cfg(test)]
mod tests {
    use super::{
        create_dock_for_presentation, is_tab_open, restore_dock_layout, serialize_dock_layout,
    };
    use crate::debug::DebugTab;
    use crate::settings::DebugPresentation;

    #[test]
    fn game_view_only_belongs_to_ide_layout() {
        let debugger = create_dock_for_presentation(DebugPresentation::GameAndDebugger);
        let floating = create_dock_for_presentation(DebugPresentation::Floating);
        let ide = create_dock_for_presentation(DebugPresentation::Ide);

        assert!(!is_tab_open(&debugger, DebugTab::GameView));
        assert!(!is_tab_open(&floating, DebugTab::GameView));
        assert!(is_tab_open(&ide, DebugTab::GameView));
    }

    #[test]
    fn debugger_layout_has_the_standard_tools() {
        let dock = create_dock_for_presentation(DebugPresentation::GameAndDebugger);

        for tab in [
            DebugTab::CpuDebug,
            DebugTab::Disassembler,
            DebugTab::MemoryViewer,
            DebugTab::Breakpoints,
            DebugTab::TileViewer,
        ] {
            assert!(is_tab_open(&dock, tab));
        }
    }

    #[test]
    fn dock_layout_round_trips_per_presentation() {
        let dock = create_dock_for_presentation(DebugPresentation::Ide);
        let saved = serialize_dock_layout(&dock).unwrap();
        let restored = restore_dock_layout(DebugPresentation::Ide, Some(&saved), &[]);

        assert_eq!(serialize_dock_layout(&restored), Some(saved));
    }

    #[test]
    fn external_layout_drops_saved_game_view() {
        let dock = create_dock_for_presentation(DebugPresentation::Ide);
        let saved = serialize_dock_layout(&dock).unwrap();
        let restored = restore_dock_layout(DebugPresentation::GameAndDebugger, Some(&saved), &[]);

        assert!(!is_tab_open(&restored, DebugTab::GameView));
    }
}
