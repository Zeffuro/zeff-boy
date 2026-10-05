use super::*;

#[test]
fn browser_window_recovers_controls_after_a_short_viewport() {
    for dpi in [1.0, 2.0] {
        let mut harness = Harness::new(egui::vec2(1280.0, 150.0), dpi);
        harness.browser_window = true;
        harness.system = ActiveSystem::MasterSystem;
        let mut state = Ui {
            active: true,
            connected: true,
            ..Ui::for_route(false)
        };
        state.open();
        harness.settle(&mut state);
        harness.screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(689.0, 892.0));
        let output = harness.settle(&mut state);
        for label in ["Pause", "Disconnect", "Message", "Stats"] {
            assert!(
                visible_text(&output, label).is_some(),
                "{label} clipped at {dpi} DPI"
            );
        }
        harness.screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(320.0, 240.0));
        harness.reach(&mut state, "Message");
        harness.reach(&mut state, "Stats");
        harness.reach(&mut state, "Disconnect");
        state.toggle_for_system(ActiveSystem::MasterSystem);
        assert!(!state.is_open());
        assert!(state.connected);
        state.toggle_for_system(ActiveSystem::MasterSystem);
        harness.reach(&mut state, "Message");
        assert!(state.connected);
    }
}
