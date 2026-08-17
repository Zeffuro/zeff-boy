mod layout;
mod presets;
mod tabs;
mod viewer;

pub(crate) use layout::{
    activate_dock_tab, create_debugger_dock_state, create_default_dock_state,
    create_ide_dock_state, ensure_game_view_tab, is_tab_open, restore_dock_layout, save_open_tabs,
    serialize_dock_layout, toggle_dock_tab,
};
pub(crate) use presets::{DebugWorkspacePreset, create_workspace_dock_state};
pub(crate) use tabs::{DebugTab, TabDataRequirements, compute_tab_requirements};
pub(crate) use viewer::DebugTabViewer;
