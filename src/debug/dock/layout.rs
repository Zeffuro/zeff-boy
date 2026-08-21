use egui_dock::DockState;

use super::tabs::DebugTab;

pub(crate) fn create_default_dock_state() -> DockState<DebugTab> {
    DockState::new(vec![])
}

pub(crate) fn create_ide_dock_state() -> DockState<DebugTab> {
    let mut dock = DockState::new(vec![DebugTab::GameView]);
    let tree = dock.main_surface_mut();

    let [center, _left] = tree.split_left(
        egui_dock::NodeIndex::root(),
        0.19,
        vec![
            DebugTab::CpuDebug,
            DebugTab::HardwareIo,
            DebugTab::Performance,
            DebugTab::ApuViewer,
            DebugTab::InputViewer,
        ],
    );

    let [_center, right] = tree.split_right(
        center,
        0.58,
        vec![
            DebugTab::Disassembler,
            DebugTab::SymbolBrowser,
            DebugTab::SourceViewer,
        ],
    );

    let [right_top, _right_bottom] = tree.split_below(
        right,
        0.7,
        vec![
            DebugTab::ExecutionHistory,
            DebugTab::Trace,
            DebugTab::CallStack,
            DebugTab::Breakpoints,
            DebugTab::Cheats,
        ],
    );

    let [_disasm, _memory] = tree.split_right(
        right_top,
        0.62,
        vec![
            DebugTab::MemoryViewer,
            DebugTab::RomViewer,
            DebugTab::RomInfo,
        ],
    );

    let [_game, _graphics] = tree.split_below(
        center,
        0.7,
        vec![
            DebugTab::OamViewer,
            DebugTab::TilemapViewer,
            DebugTab::TileViewer,
            DebugTab::PaletteViewer,
        ],
    );

    dock
}

pub(crate) fn create_debugger_dock_state() -> DockState<DebugTab> {
    let mut dock = DockState::new(vec![
        DebugTab::Disassembler,
        DebugTab::SourceViewer,
        DebugTab::SymbolBrowser,
    ]);
    let tree = dock.main_surface_mut();

    let [center, _left] = tree.split_left(
        egui_dock::NodeIndex::root(),
        0.2,
        vec![
            DebugTab::CpuDebug,
            DebugTab::HardwareIo,
            DebugTab::Performance,
            DebugTab::ApuViewer,
            DebugTab::InputViewer,
        ],
    );

    let [top, bottom] = tree.split_below(
        center,
        0.67,
        vec![
            DebugTab::ExecutionHistory,
            DebugTab::Trace,
            DebugTab::CallStack,
            DebugTab::Breakpoints,
            DebugTab::Cheats,
        ],
    );

    let [_disasm, _memory] = tree.split_right(
        top,
        0.64,
        vec![
            DebugTab::MemoryViewer,
            DebugTab::RomViewer,
            DebugTab::RomInfo,
        ],
    );

    let [_history, _graphics] = tree.split_right(
        bottom,
        0.55,
        vec![
            DebugTab::OamViewer,
            DebugTab::TilemapViewer,
            DebugTab::TileViewer,
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
        toggle_dock_tab,
    };
    use crate::debug::{DebugTab, compute_tab_requirements};
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
    fn floating_layout_starts_without_debug_tools() {
        let mut dock = create_dock_for_presentation(DebugPresentation::Floating);
        let requirements = compute_tab_requirements(&dock);

        assert!(!is_tab_open(&dock, DebugTab::CpuDebug));
        assert!(!requirements.needs_debug_info);
        assert!(!requirements.needs_disassembly);
        assert!(!requirements.needs_memory_page);
        assert!(!requirements.needs_viewer_data);

        toggle_dock_tab(&mut dock, DebugTab::CpuDebug);
        assert!(is_tab_open(&dock, DebugTab::CpuDebug));
        assert!(compute_tab_requirements(&dock).needs_debug_info);
    }

    #[test]
    fn debugger_layout_has_the_standard_tools() {
        let dock = create_dock_for_presentation(DebugPresentation::GameAndDebugger);

        for tab in [
            DebugTab::CpuDebug,
            DebugTab::HardwareIo,
            DebugTab::Disassembler,
            DebugTab::MemoryViewer,
            DebugTab::SourceViewer,
            DebugTab::ExecutionHistory,
            DebugTab::Trace,
            DebugTab::CallStack,
            DebugTab::Breakpoints,
            DebugTab::TileViewer,
        ] {
            assert!(is_tab_open(&dock, tab));
        }
    }

    #[test]
    fn debugger_default_exposes_multiple_live_panes() {
        let dock = create_dock_for_presentation(DebugPresentation::GameAndDebugger);
        let requirements = compute_tab_requirements(&dock);

        assert!(requirements.needs_debug_info);
        assert!(requirements.needs_disassembly);
        assert!(requirements.needs_memory_page);
        assert!(requirements.needs_oam);
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
