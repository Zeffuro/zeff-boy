use super::*;

fn test_addresses() -> Vec<crate::netplay::adapters::HostAddress> {
    [
        ("Ethernet", "192.168.1.10", true),
        ("Ethernet", "192.168.1.11", false),
        ("Loopback", "127.0.0.1", false),
    ]
    .map(
        |(adapter, address, recommended)| crate::netplay::adapters::HostAddress {
            adapter: adapter.into(),
            address: address.parse().unwrap(),
            recommended,
        },
    )
    .into()
}

struct Harness {
    ctx: egui::Context,
    screen: egui::Rect,
    dpi: f32,
    system: ActiveSystem,
}

impl Harness {
    fn new(size: egui::Vec2, dpi: f32) -> Self {
        Self {
            ctx: egui::Context::default(),
            screen: egui::Rect::from_min_size(egui::Pos2::ZERO, size),
            dpi,
            system: ActiveSystem::Nes,
        }
    }

    fn frame(
        &self,
        state: &mut Ui,
        events: Vec<egui::Event>,
    ) -> (egui::FullOutput, Option<MenuAction>) {
        if !state.host_addresses.is_ready() {
            state
                .host_addresses
                .supply(test_addresses(), &mut state.host_address);
        }
        let mut input = egui::RawInput {
            screen_rect: Some(self.screen),
            events,
            ..Default::default()
        };
        input
            .viewports
            .get_mut(&egui::ViewportId::ROOT)
            .unwrap()
            .native_pixels_per_point = Some(self.dpi);
        let mut action = None;
        let output = self.ctx.run_ui(input, |ui| {
            if state.is_open() {
                action = state.draw_contents(ui, self.system);
            }
        });
        (output, action)
    }

    fn settle(&self, state: &mut Ui) -> egui::FullOutput {
        self.frame(state, vec![]);
        self.frame(state, vec![]);
        self.frame(state, vec![]).0
    }

    fn click(&self, state: &mut Ui, pos: egui::Pos2) -> (egui::FullOutput, Option<MenuAction>) {
        self.frame(state, vec![egui::Event::PointerMoved(pos)]);
        self.frame(
            state,
            vec![egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: Default::default(),
            }],
        );
        self.frame(
            state,
            vec![egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: Default::default(),
            }],
        )
    }

    fn reach(&self, state: &mut Ui, label: &str) -> egui::Rect {
        for attempt in 0..16 {
            let output = self.settle(state);
            if let Some(rect) = visible_text(&output, label) {
                assert!(self.screen.contains_rect(rect), "{label}: {rect:?}");
                return rect;
            }
            let pos = self.screen.center();
            self.frame(
                state,
                vec![
                    egui::Event::PointerMoved(pos),
                    egui::Event::MouseWheel {
                        unit: egui::MouseWheelUnit::Point,
                        delta: egui::vec2(0.0, if attempt < 8 { -80.0 } else { 80.0 }),
                        phase: egui::TouchPhase::Move,
                        modifiers: Default::default(),
                    },
                ],
            );
        }
        panic!(
            "{label} is unreachable in {:?} at {} DPI",
            self.screen, self.dpi
        );
    }
}

fn visible_text(output: &egui::FullOutput, label: &str) -> Option<egui::Rect> {
    fn find(shape: &egui::Shape, clip: egui::Rect, label: &str) -> Option<egui::Rect> {
        match shape {
            egui::Shape::Text(text) if text.galley.job.text == label => {
                let rect = text.visual_bounding_rect();
                clip.contains_rect(rect).then_some(rect)
            }
            egui::Shape::Vec(shapes) => shapes.iter().find_map(|shape| find(shape, clip, label)),
            _ => None,
        }
    }
    output
        .shapes
        .iter()
        .find_map(|shape| find(&shape.shape, shape.clip_rect, label))
}

#[test]
fn host_and_join_are_clickable_without_an_open_menu_at_small_viewports() {
    for (size, dpi) in [
        (egui::vec2(320.0, 240.0), 1.0),
        (egui::vec2(320.0, 240.0), 2.0),
        (egui::vec2(640.0, 360.0), 2.0),
    ] {
        let harness = Harness::new(size, dpi);
        let mut state = Ui {
            private_network: true,
            host_address: "192.168.1.10".into(),
            ..Default::default()
        };
        state.open();
        let host = harness.reach(&mut state, "Start hosting");
        assert!(matches!(
            harness.click(&mut state, host.center()).1,
            Some(MenuAction::HostNesNetplay)
        ));
        let join = harness.reach(&mut state, "Join");
        harness.click(&mut state, join.center());
        assert!(state.joining);
        state.join = "  secret invitation  ".into();
        let join = harness.reach(&mut state, "Connect");
        assert!(
            matches!(harness.click(&mut state, join.center()).1, Some(MenuAction::JoinNesNetplay(invitation)) if invitation == "secret invitation")
        );
        assert!(state.private_network);
        assert_eq!(state.host_address, "192.168.1.10");
    }
}

#[test]
fn lobby_and_link_actions_remain_reachable_in_compact_window() {
    for dpi in [1.0, 2.0] {
        let mut harness = Harness::new(egui::vec2(320.0, 240.0), dpi);
        let mut state = Ui {
            lobby: true,
            ..Ui::for_route(false)
        };
        state.open();
        harness.reach(&mut state, "Server");
        harness.reach(&mut state, "Key");
        let host = harness.reach(&mut state, "Start hosting");
        assert!(matches!(
            harness.click(&mut state, host.center()).1,
            Some(MenuAction::HostNesNetplay)
        ));
        for system in [ActiveSystem::GameBoy, ActiveSystem::WonderSwan] {
            harness.system = system;
            state.open_for_system(system);
            let host = harness.reach(&mut state, "Host");
            assert!(matches!(
                harness.click(&mut state, host.center()).1,
                Some(MenuAction::HostTcpLink)
            ));
            let join = harness.reach(&mut state, "Join");
            assert!(matches!(
                harness.click(&mut state, join.center()).1,
                Some(MenuAction::JoinTcpLink)
            ));
            state.link_active = true;
            let disconnect = harness.reach(&mut state, "Disconnect");
            assert!(matches!(
                harness.click(&mut state, disconnect.center()).1,
                Some(MenuAction::DisconnectLink)
            ));
            state.link_active = false;
        }
    }
}

#[test]
fn pending_and_connected_session_actions_stay_reachable() {
    let harness = Harness::new(egui::vec2(320.0, 240.0), 2.0);
    let mut state = Ui {
        active: true,
        invitation: "secret invitation".into(),
        status: "Waiting for the other player…".into(),
        ..Default::default()
    };
    state.open();
    let copy = harness.reach(&mut state, "Copy invitation");
    let output = harness.click(&mut state, copy.center()).0;
    assert!(output.platform_output.commands.iter().any(|command| matches!(command, egui::OutputCommand::CopyText(text) if text == "secret invitation")));
    let cancel = harness.reach(&mut state, "Cancel");
    assert!(matches!(
        harness.click(&mut state, cancel.center()).1,
        Some(MenuAction::StopNesNetplay)
    ));
    state.connected = true;
    state.invitation.clear();
    let pause = harness.reach(&mut state, "Pause");
    assert!(matches!(
        harness.click(&mut state, pause.center()).1,
        Some(MenuAction::SetNesNetplayPaused(true))
    ));
    state.local_pause = true;
    let release = harness.reach(&mut state, "Resume");
    assert!(matches!(
        harness.click(&mut state, release.center()).1,
        Some(MenuAction::SetNesNetplayPaused(false))
    ));
    let disconnect = harness.reach(&mut state, "Disconnect");
    assert!(matches!(
        harness.click(&mut state, disconnect.center()).1,
        Some(MenuAction::StopNesNetplay)
    ));
}

#[test]
fn closing_and_reopening_keeps_session_and_form_state() {
    let harness = Harness::new(egui::vec2(640.0, 480.0), 1.0);
    let mut state = Ui {
        active: true,
        connected: true,
        local_pause: true,
        allow_different_versions: true,
        private_network: true,
        host_address: "192.168.1.10".into(),
        host_port: 12345,
        input_delay: 8,
        joining: true,
        join: "invitation draft".into(),
        ..Default::default()
    };
    state.open();
    assert!(state.take_focus_request());
    assert!(!state.take_focus_request());
    harness.settle(&mut state);
    state.set_host_window_focused(true);
    state.open();
    assert!(state.take_focus_request());
    state.open();
    state.close();
    assert!(!state.is_open());
    assert!(!state.host_window_focused());
    assert!(!state.take_focus_request());
    assert!(state.active && state.connected && state.local_pause);
    assert!(state.allow_different_versions);
    assert!(state.private_network && state.joining);
    assert_eq!(state.host_address, "192.168.1.10");
    assert_eq!(state.host_port, 12345);
    assert_eq!(state.input_delay, 8);
    assert_eq!(state.join, "invitation draft");
    assert!(visible_text(&harness.settle(&mut state), "Resume").is_none());
    state.open();
    assert!(state.take_focus_request());
    harness.reach(&mut state, "Resume");
    assert!(state.active && state.connected && state.local_pause);
}

#[test]
fn failure_status_and_expanded_details_can_be_scrolled_into_view() {
    let harness = Harness::new(egui::vec2(320.0, 240.0), 2.0);
    let mut state = Ui {
        private_network: true,
        allow_different_versions: true,
        status: "Connection failed. Check the host address and try again.".into(),
        ..Default::default()
    };
    state.open();
    let status = state.status.clone();
    harness.reach(&mut state, &status);
    let details = harness.reach(&mut state, "Details");
    harness.click(&mut state, details.center());
    harness.reach(
        &mut state,
        "Player 1 controls on both devices. Click the game to play.",
    );
    harness.reach(&mut state, "Both players must resume to continue.");
    harness.reach(&mut state, "Disconnect resets the game and pauses it.");
    harness.reach(
        &mut state,
        "Use a trusted LAN. The host address must be reachable.",
    );
    harness.reach(&mut state, "Start hosting");
    assert!(state.allow_different_versions);
}

#[test]
fn host_join_and_connected_delay_are_visible_at_small_viewports() {
    for dpi in [1.0, 2.0] {
        let harness = Harness::new(egui::vec2(320.0, 240.0), dpi);
        let mut state = Ui {
            input_delay: 8,
            ..Default::default()
        };
        state.open();
        harness.reach(&mut state, "Delay (frames)");
        state.joining = true;
        state.join = format!("127.0.0.1:8766/{}/4", "ab".repeat(32));
        harness.reach(&mut state, "Host delay: 4 frames");
        assert_eq!(state.input_delay, 8);
        state.active = true;
        state.connected = true;
        state.input_delay = 4;
        harness.reach(&mut state, "Delay: 4 frames");
    }
}

#[test]
fn scopes_and_start_requirements_remain_interactive() {
    let mut harness = Harness::new(egui::vec2(320.0, 240.0), 2.0);
    let mut state = Ui::default();
    state.open();
    let lan = harness.reach(&mut state, "LAN");
    harness.click(&mut state, lan.center());
    assert_eq!(state.scope(), ConnectionScope::TrustedPrivate);
    harness.reach(&mut state, "Adapter");
    let options = harness.reach(&mut state, "Options");
    harness.click(&mut state, options.center());
    harness.reach(&mut state, "Port");
    let same_pc = harness.reach(&mut state, "Same PC");
    harness.click(&mut state, same_pc.center());
    assert_eq!(state.scope(), ConnectionScope::Loopback);
    let join = harness.reach(&mut state, "Join");
    harness.click(&mut state, join.center());
    let connect = harness.reach(&mut state, "Connect");
    assert!(harness.click(&mut state, connect.center()).1.is_none());
    state.join = "  \n ".into();
    let connect = harness.reach(&mut state, "Connect");
    assert!(harness.click(&mut state, connect.center()).1.is_none());
    harness.system = ActiveSystem::GameBoy;
    state.joining = false;
    let host = harness.reach(&mut state, "Start hosting");
    assert!(harness.click(&mut state, host.center()).1.is_none());
    harness.reach(
        &mut state,
        "Shared-console netplay is currently available for NES.",
    );
}

#[test]
fn session_metrics_and_secret_are_hidden_until_details_open() {
    let harness = Harness::new(egui::vec2(320.0, 240.0), 2.0);
    let mut state = Ui {
        active: true,
        metrics: "Rollback statistics".into(),
        invitation: "secret invitation".into(),
        ..Default::default()
    };
    state.open();
    let output = harness.settle(&mut state);
    assert!(visible_text(&output, &state.metrics).is_none());
    assert!(visible_text(&output, &state.invitation).is_none());
    let details = harness.reach(&mut state, "Details");
    harness.click(&mut state, details.center());
    harness.reach(&mut state, "Rollback statistics");
    harness.reach(&mut state, "Share this secret only with the other player:");
    assert_eq!(state.invitation, "secret invitation");
}

#[test]
fn adapter_picker_selects_assigned_ip_and_custom_fallback() {
    for dpi in [1.0, 2.0] {
        let harness = Harness::new(egui::vec2(640.0, 480.0), dpi);
        let mut state = Ui {
            private_network: true,
            ..Ui::for_route(false)
        };
        state.open();
        let selected = harness.reach(&mut state, "Ethernet — 192.168.1.10 (Recommended)");
        harness.click(&mut state, selected.center());
        let alternate = harness.reach(&mut state, "Ethernet — 192.168.1.11");
        harness.click(&mut state, alternate.center());
        assert_eq!(
            state.host_options().unwrap().address.ip().to_string(),
            "192.168.1.11"
        );
        state.close();
        state.open();
        let selected = harness.reach(&mut state, "Ethernet — 192.168.1.11");
        harness.click(&mut state, selected.center());
        let custom = harness.reach(&mut state, "Custom IP");
        harness.click(&mut state, custom.center());
        state.host_address = "fd00::123".into();
        assert_eq!(
            state.host_options().unwrap().address.ip().to_string(),
            "fd00::123"
        );
    }
}

#[test]
fn removed_adapter_disables_hosting_until_reselected() {
    let harness = Harness::new(egui::vec2(320.0, 240.0), 2.0);
    let mut state = Ui {
        private_network: true,
        ..Ui::for_route(false)
    };
    state.open();
    harness.settle(&mut state);
    state.host_addresses.supply(vec![], &mut state.host_address);
    let host = harness.reach(&mut state, "Start hosting");
    assert!(harness.click(&mut state, host.center()).1.is_none());
    assert!(state.host_options().is_err());
}

#[test]
fn long_adapter_names_and_ipv6_remain_clickable_in_small_popups() {
    for dpi in [1.0, 2.0] {
        let harness = Harness::new(egui::vec2(320.0, 240.0), dpi);
        let mut state = Ui {
            private_network: true,
            ..Ui::for_route(false)
        };
        let address = "fd12:3456:789a:bcde:1234:5678:9abc:def0";
        let mut choices = test_addresses();
        choices.push(crate::netplay::adapters::HostAddress {
            adapter: "A very long friendly adapter name with several words".into(),
            address: address.parse().unwrap(),
            recommended: false,
        });
        state
            .host_addresses
            .supply(choices, &mut state.host_address);
        state.open();
        let label = format!("A very long friendly adapter name with several words — {address}");
        assert_eq!(state.host_address, "192.168.1.10");
        let selected = harness.reach(&mut state, "Ethernet — 192.168.1.10 (Recommended)");
        harness.click(&mut state, selected.center());
        let choice = harness.reach(&mut state, &label);
        harness.click(&mut state, choice.center());
        assert_eq!(
            state.host_options().unwrap().address.ip().to_string(),
            address
        );
        harness.reach(&mut state, "Refresh");
        harness.reach(&mut state, "Start hosting");
    }
}

#[test]
fn status_has_one_primary_line_and_options_hide_technical_controls() {
    let harness = Harness::new(egui::vec2(640.0, 480.0), 1.0);
    let mut state = Ui {
        active: true,
        connected: true,
        status: "Paused by the other player".into(),
        ..Ui::for_route(false)
    };
    state.open();
    let output = harness.settle(&mut state);
    assert!(visible_text(&output, "Paused by the other player").is_some());
    assert!(visible_text(&output, "Connected").is_none());
    state.active = false;
    state.connected = false;
    state.private_network = true;
    let output = harness.settle(&mut state);
    assert!(visible_text(&output, "Port").is_none());
    let options = harness.reach(&mut state, "Options");
    harness.click(&mut state, options.center());
    harness.reach(&mut state, "Port");
    assert!(!state.allow_different_versions);
}

#[test]
fn refresh_stays_on_one_line_beside_the_adapter_at_small_viewports() {
    for dpi in [1.0, 2.0] {
        let harness = Harness::new(egui::vec2(320.0, 240.0), dpi);
        let mut state = Ui {
            private_network: true,
            ..Ui::for_route(false)
        };
        state.open();
        let output = harness.settle(&mut state);
        let refresh = visible_text(&output, "Refresh").unwrap();
        let adapter = visible_text(&output, "Ethernet — 192.168.1.10 (Recommended)").unwrap();
        assert!(refresh.height() < 20.0, "Refresh wrapped: {refresh:?}");
        assert!(
            (refresh.center().y - adapter.center().y).abs() < 4.0,
            "Refresh {refresh:?}, adapter {adapter:?}"
        );
        assert!(harness.screen.contains_rect(refresh));
        assert!(refresh.left() > adapter.left());
    }
}

#[test]
fn chat_sends_trimmed_text_and_rejects_empty_or_oversize_messages() {
    for dpi in [1.0, 2.0] {
        let harness = Harness::new(egui::vec2(320.0, 240.0), dpi);
        let mut state = Ui {
            active: true,
            connected: true,
            ..Ui::for_route(false)
        };
        state.open();
        for draft in ["  ".to_owned(), "🕹".repeat(129)] {
            state.chat_draft = draft.clone();
            let send = harness.reach(&mut state, "Send");
            assert!(harness.click(&mut state, send.center()).1.is_none());
            assert_eq!(state.chat_draft, draft);
        }
        state.chat_draft = "  Hello!  ".into();
        let send = harness.reach(&mut state, "Send");
        assert!(matches!(harness.click(&mut state, send.center()).1,
            Some(MenuAction::SendNesNetplayChat(text)) if text == "Hello!"));
        assert_eq!(state.chat_draft, "  Hello!  ");
        assert!(state.chat_pending);
        state.chat_sent("Hello!");
        assert!(state.chat_draft.is_empty());
        assert!(!state.chat_pending);
        state.connected = false;
        state.chat.push(false, "Hi!".into());
        let output = harness.settle(&mut state);
        assert!(visible_text(&output, "Send").is_none());
        harness.reach(&mut state, "Other player: Hi!");
        harness.reach(&mut state, "Chat is available while connected.");
    }
}

#[test]
fn chat_accepts_enter_and_keeps_composer_focus() {
    let harness = Harness::new(egui::vec2(320.0, 240.0), 2.0);
    let mut state = Ui {
        active: true,
        connected: true,
        ..Ui::for_route(false)
    };
    state.open();
    let composer = harness.reach(&mut state, "Message");
    harness.click(&mut state, composer.center());
    harness.frame(
        &mut state,
        vec![egui::Event::Text("Hello from Enter".into())],
    );
    let action = harness
        .frame(
            &mut state,
            vec![egui::Event::Key {
                key: egui::Key::Enter,
                physical_key: Some(egui::Key::Enter),
                pressed: true,
                repeat: false,
                modifiers: Default::default(),
            }],
        )
        .1;
    assert!(
        matches!(action, Some(MenuAction::SendNesNetplayChat(text)) if text == "Hello from Enter")
    );
    assert_eq!(state.chat_draft, "Hello from Enter");
    state.chat_sent("Hello from Enter");
    assert!(state.chat_draft.is_empty());
    harness.frame(
        &mut state,
        vec![
            egui::Event::Key {
                key: egui::Key::Enter,
                physical_key: Some(egui::Key::Enter),
                pressed: false,
                repeat: false,
                modifiers: Default::default(),
            },
            egui::Event::Text("Again".into()),
        ],
    );
    assert_eq!(state.chat_draft, "Again");
}

#[test]
fn chat_pending_rejection_retry_and_ack_preserve_user_drafts() {
    let harness = Harness::new(egui::vec2(320.0, 240.0), 2.0);
    let mut state = Ui {
        active: true,
        connected: true,
        ..Ui::for_route(false)
    };
    state.open();
    state.chat_draft = "hello".into();
    let send = harness.reach(&mut state, "Send");
    assert!(matches!(harness.click(&mut state, send.center()).1,
        Some(MenuAction::SendNesNetplayChat(text)) if text == "hello"));
    let send = harness.reach(&mut state, "Send");
    assert!(harness.click(&mut state, send.center()).1.is_none());
    assert_eq!(state.chat_draft, "hello");
    state.chat_failed("Wait a moment before sending again.".into());
    harness.reach(&mut state, "Wait a moment before sending again.");
    state.chat.push(false, "Hi!".into());
    harness.reach(&mut state, "Wait a moment before sending again.");
    assert!(!state.chat_pending);
    assert_eq!(state.chat_draft, "hello");
    let send = harness.reach(&mut state, "Send");
    assert!(matches!(harness.click(&mut state, send.center()).1,
        Some(MenuAction::SendNesNetplayChat(text)) if text == "hello"));
    state.chat_draft = "next message".into();
    state.chat_sent("hello");
    assert!(!state.chat_pending);
    assert_eq!(state.chat_draft, "next message");
    assert!(state.chat.error.is_empty());
    state.chat_pending = true;
    state.chat.push(true, "hello".into());
    state.clear_chat();
    assert!(state.chat_draft.is_empty());
    assert!(!state.chat_pending);
    assert_eq!(state.chat.messages().count(), 0);
    state.chat_draft = "hidden\u{202e}text".into();
    let send = harness.reach(&mut state, "Send");
    assert!(harness.click(&mut state, send.center()).1.is_none());
}
