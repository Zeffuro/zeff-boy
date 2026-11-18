use super::*;
use crate::emu_thread::{MdTimedPortFailure, MdTimedPortFailureKind};

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn md_port_editor_keeps_full_words_and_invalid_input_does_not_change_ordered_batch() {
    let mut state = MdTimedPortUi::default();
    state.word(0xc00004, "$8F_02").unwrap();
    state.word(0xc00004, "0x4000").unwrap();
    state.word(0xc00000, "00A5").unwrap();
    state.draft.push(DebugPortWrite::Byte {
        address: 0xa11100,
        value: 1,
    });
    state.draft.push(DebugPortWrite::Byte {
        address: 0xa11200,
        value: 0,
    });
    let expected = vec![
        DebugPortWrite::Word {
            address: 0xc00004,
            value: 0x8f02,
        },
        DebugPortWrite::Word {
            address: 0xc00004,
            value: 0x4000,
        },
        DebugPortWrite::Word {
            address: 0xc00000,
            value: 0xa5,
        },
        DebugPortWrite::Byte {
            address: 0xa11100,
            value: 1,
        },
        DebugPortWrite::Byte {
            address: 0xa11200,
            value: 0,
        },
    ];
    for invalid in ["", "10000", "-1", "GG", "0x"] {
        assert!(state.word(0xc00004, invalid).is_err());
        assert_eq!(state.draft, expected);
    }
    assert_eq!(state.prepare().unwrap().writes, expected);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn md_port_editor_correlates_results_defers_authority_and_cancels_old_session_actions() {
    let mut state = MdTimedPortUi::default();
    state.word(0xc00004, "8F02").unwrap();
    let first = state.prepare().unwrap();
    state.queue(first.clone()).unwrap();
    assert!(state.queue(first.clone()).is_err());
    state.sent(first.id);
    let mut actions = DebugUiActions::none();
    actions.memory_writes.push((0xff0041, 0xa5));
    state.defer(actions);
    let mut actions = DebugUiActions::none();
    actions.continue_requested = true;
    state.defer(actions);
    assert!(state.take_deferred().is_none());
    let reply = MdTimedPortReply {
        id: first.id,
        result: Err(MdTimedPortFailure {
            kind: MdTimedPortFailureKind::Port,
            index: Some(2),
            reason: "literal rejected write".into(),
        }),
        queued_pcm_samples_before: Some(4),
        delivered_pcm_samples: 4,
    };
    let mut stale = reply.clone();
    stale.id = 0;
    assert!(!state.receive(stale));
    assert!(state.busy());
    assert!(state.receive(reply.clone()));
    assert!(
        state
            .status()
            .contains("Port at write 2: literal rejected write")
    );
    assert!(!state.can_queue());
    assert!(state.ordering_pending());
    assert!(state.take_deferred().is_none());
    state.ordinary_delivery();
    assert!(state.take_deferred().is_none());
    assert!(state.ordinary_flush_required());
    state.ordinary_flush_sent();
    assert!(!state.ordinary_flush_required());
    assert!(state.take_deferred().is_none());
    state.ordinary_delivery();
    assert!(state.ordering_pending());
    assert_eq!(
        state.take_deferred().unwrap().memory_writes,
        [(0xff0041, 0xa5)]
    );
    assert!(state.take_deferred().is_none());
    assert!(state.ordinary_flush_required());
    state.ordinary_flush_sent();
    assert!(state.take_deferred().is_none());
    state.ordinary_delivery();
    assert!(state.take_deferred().unwrap().continue_requested);
    assert!(!state.ordering_pending());
    let second = state.prepare().unwrap();
    assert!(second.id > first.id);
    state.queue(second.clone()).unwrap();
    state.sent(second.id);
    let mut actions = DebugUiActions::none();
    actions.backstep_requested = true;
    state.defer(actions);
    state.cancel_session("Session changed");
    assert!(!state.busy() && state.take_deferred().is_none());
    assert!(state.draft.is_empty());
    assert!(!state.receive(MdTimedPortReply {
        id: second.id,
        ..reply
    }));
    state.word(0xc00004, "8F04").unwrap();
    let third = state.prepare().unwrap();
    assert!(third.id > second.id);
    state.queue(third).unwrap();
    state.send_failed("channel closed".into());
    assert!(!state.busy());
    assert!(state.queued().is_none());
    assert_eq!(state.status(), "channel closed");
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn md_psg_editor_preserves_latch_data_bytes_and_rejects_overflow() {
    let mut state = MdTimedPortUi::default();
    for text in ["85", "0x12", "$90"] {
        state.psg_byte(text).unwrap();
    }
    let expected: Vec<_> = [0x85, 0x12, 0x90]
        .into_iter()
        .map(|value| DebugPortWrite::Byte {
            address: 0xc00011,
            value,
        })
        .collect();
    for invalid in ["", "100", "0x100", "-1", "GG"] {
        assert!(state.psg_byte(invalid).is_err());
        assert_eq!(state.draft, expected);
    }
    assert_eq!(state.prepare().unwrap().writes, expected);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn md_sound_aperture_editor_keeps_canonical_byte_addresses_and_rejects_mirrors() {
    let mut state = MdTimedPortUi::default();
    for address in [
        "A00000", "A00001", "A01FFE", "A01FFF", "A04000", "A04001", "A04002", "A04003", "A06000",
    ] {
        state.sound_byte(address, "A5").unwrap();
    }
    let expected: Vec<_> = [
        0xa00000, 0xa00001, 0xa01ffe, 0xa01fff, 0xa04000, 0xa04001, 0xa04002, 0xa04003, 0xa06000,
    ]
    .into_iter()
    .map(|address| DebugPortWrite::Byte {
        address,
        value: 0xa5,
    })
    .collect();
    assert_eq!(state.draft, expected);
    assert_eq!(expected.len(), 9);
    assert_eq!(
        expected[0],
        DebugPortWrite::Byte {
            address: 0xa00000,
            value: 0xa5
        }
    );
    assert_eq!(
        expected[3],
        DebugPortWrite::Byte {
            address: 0xa01fff,
            value: 0xa5
        }
    );
    assert_eq!(
        expected[8],
        DebugPortWrite::Byte {
            address: 0xa06000,
            value: 0xa5
        }
    );
    for address in [
        "",
        "A02000",
        "A04004",
        "A06001",
        "A07F11",
        "1A00000",
        "FF0000",
        "100A00000",
    ] {
        assert!(state.sound_byte(address, "5A").is_err());
        assert_eq!(state.draft, expected);
    }
    for value in ["", "100", "-1", "GG"] {
        assert!(state.sound_byte("A00000", value).is_err());
        assert_eq!(state.draft, expected);
    }
    assert_eq!(state.prepare().unwrap().writes, expected);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn md_uart_editor_preserves_canonical_lower_bytes_and_rejects_wide_addresses() {
    let mut state = MdTimedPortUi::default();
    for (address, value) in [
        ("A10013", "38"),
        ("A1000F", "53"),
        ("A10019", "38"),
        ("A10015", "4D"),
    ] {
        state.uart_byte(address, value).unwrap();
    }
    let expected = vec![
        DebugPortWrite::Byte {
            address: 0xa10013,
            value: 0x38,
        },
        DebugPortWrite::Byte {
            address: 0xa1000f,
            value: 0x53,
        },
        DebugPortWrite::Byte {
            address: 0xa10019,
            value: 0x38,
        },
        DebugPortWrite::Byte {
            address: 0xa10015,
            value: 0x4d,
        },
    ];
    assert_eq!(state.draft, expected);
    for address in [
        "",
        "A1000E",
        "A10011",
        "A10012",
        "A10017",
        "A1001B",
        "A1001F",
        "A1002F",
        "1A1000F",
        "100A1000F",
    ] {
        assert!(state.uart_byte(address, "53").is_err());
        assert_eq!(state.draft, expected);
    }
    for value in ["", "100", "-1", "GG"] {
        assert!(state.uart_byte("A1000F", value).is_err());
        assert_eq!(state.draft, expected);
    }
    assert_eq!(state.prepare().unwrap().writes, expected);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn md_save_editor_requires_current_profile_and_preserves_checked_byte_scope() {
    let ports = zeff_md_core::emulator::DebugSavePorts {
        data_start: 0x200000,
        data_end: 0x203fff,
        control: 0xa130f1,
        enabled: true,
        write_protected: false,
    };
    let mut state = MdTimedPortUi::default();
    for (address, value) in [
        ("200001", "A5"),
        ("200000", "5A"),
        ("203FFF", "00"),
        ("A130F1", "03"),
        ("A130F1", "01"),
    ] {
        state.save_byte(Some(ports), address, value).unwrap();
    }
    let expected = state.draft.clone();
    assert_eq!(expected.len(), 5);
    assert_eq!(
        expected[0],
        DebugPortWrite::Byte {
            address: 0x200001,
            value: 0xa5
        }
    );
    assert_eq!(
        expected[3],
        DebugPortWrite::Byte {
            address: 0xa130f1,
            value: 3
        }
    );
    for address in [
        "",
        "1FFFFF",
        "204000",
        "A130F0",
        "A130F3",
        "1200001",
        "100200001",
    ] {
        assert!(state.save_byte(Some(ports), address, "A5").is_err());
        assert_eq!(state.draft, expected);
    }
    for value in ["", "100", "-1", "GG"] {
        assert!(state.save_byte(Some(ports), "200001", value).is_err());
        assert_eq!(state.draft, expected);
    }
    assert!(state.save_byte(None, "200001", "A5").is_err());
    assert_eq!(state.draft, expected);
    assert_eq!(state.prepare().unwrap().writes, expected);
}
