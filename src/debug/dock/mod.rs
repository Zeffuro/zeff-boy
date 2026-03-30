mod layout;
mod tabs;
mod viewer;

pub(crate) use layout::{
    create_default_dock_state, create_dock_from_saved_tabs, create_ide_dock_state,
    ensure_game_view_tab, is_tab_open, save_open_tabs, toggle_dock_tab,
};
pub(crate) use tabs::{DebugTab, compute_tab_requirements};
pub(crate) use viewer::DebugTabViewer;

