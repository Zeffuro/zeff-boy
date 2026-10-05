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

#[test]
fn browser_join_recovers_connect_after_an_expired_invitation_and_resize() {
    for dpi in [1.0, 2.0] {
        let mut harness = Harness::new(egui::vec2(1280.0, 150.0), dpi);
        harness.browser_window = true;
        harness.system = ActiveSystem::MasterSystem;
        let mut state = Ui {
            lobby: true,
            joining: true,
            status: "Join failed: invitation expired.\nPaste a new invitation to try again.".into(),
            ..Ui::for_route(false)
        };
        state.open();
        harness.settle(&mut state);
        harness.screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(689.0, 892.0));
        let invitation = crate::netplay::connect::lobby::invitation(
            &state.lobby_url,
            &"a".repeat(24),
            [7; 32],
            InputDelay::new(2).unwrap(),
        );
        state.join = invitation.clone();
        assert_eq!(state.invitation_delay(&invitation).unwrap().frames(), 2);
        let output = harness.settle(&mut state);
        for label in [&state.status[..], "Host delay: 2 frames", "Connect"] {
            let rect = visible_text(&output, label)
                .unwrap_or_else(|| panic!("{label} clipped at {dpi} DPI"));
            assert!(harness.screen.contains_rect(rect));
        }
        let connect = visible_text(&output, "Connect").unwrap();
        assert!(matches!(
            harness.click(&mut state, connect.center()).1,
            Some(MenuAction::JoinNesNetplay(value)) if value == invitation
        ));
        harness.screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(320.0, 240.0));
        let connect = harness.reach(&mut state, "Connect");
        assert!(matches!(
            harness.click(&mut state, connect.center()).1,
            Some(MenuAction::JoinNesNetplay(value)) if value == invitation
        ));
        assert_eq!(state.join, invitation);
        assert!(!state.active && !state.connected);
    }
}
