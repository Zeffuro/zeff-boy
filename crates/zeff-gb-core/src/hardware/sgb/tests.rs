use super::{SgbEvent, SgbState};

fn feed_packet(state: &mut SgbState, packet: [u8; 16]) -> Option<SgbEvent> {
    let mut out = state.on_joyp_write(0x00);
    for byte in packet {
        for bit in 0..8 {
            let value = if (byte >> bit) & 1 == 0 { 0x20 } else { 0x10 };
            out = state.on_joyp_write(value);
        }
    }
    out
}

#[test]
fn decodes_pal01_packet() {
    let mut state = SgbState::new();
    let mut packet = [0u8; 16];
    packet[0] = 0x01;
    packet[1] = 0xFF;
    packet[2] = 0x7F;
    packet[3] = 0x00;
    packet[4] = 0x00;

    let event = feed_packet(&mut state, packet).expect("expected PAL01 event");
    match event {
        SgbEvent::Pal01(p0, p1) => {
            assert_eq!(p0[0], 0x7FFF);
            assert_eq!(p1[0], 0x7FFF);
        }
        _ => panic!("unexpected event"),
    }
}

#[test]
fn unknown_command_is_ignored() {
    let mut state = SgbState::new();
    let mut packet = [0u8; 16];
    packet[0] = (0x1E << 3) | 0x01;
    assert!(feed_packet(&mut state, packet).is_none());
}

#[test]
fn decodes_chr_trn_bank() {
    let mut state = SgbState::new();
    let mut packet = [0u8; 16];
    packet[0] = (0x13 << 3) | 0x01;
    packet[1] = 0x01;

    let event = feed_packet(&mut state, packet).expect("expected CHR_TRN event");
    match event {
        SgbEvent::ChrTrn(bank) => assert_eq!(bank, 1),
        _ => panic!("unexpected event"),
    }
}

#[test]
fn decodes_pct_trn() {
    let mut state = SgbState::new();
    let mut packet = [0u8; 16];
    packet[0] = (0x14 << 3) | 0x01;

    let event = feed_packet(&mut state, packet).expect("expected PCT_TRN event");
    assert!(matches!(event, SgbEvent::PctTrn));
}

#[test]
fn decodes_pal_trn() {
    let mut state = SgbState::new();
    let mut packet = [0u8; 16];
    packet[0] = (0x0B << 3) | 0x01;

    let event = feed_packet(&mut state, packet).expect("expected PAL_TRN event");
    assert!(matches!(event, SgbEvent::PalTrn));
}

#[test]
fn does_not_drop_multi_packet_command_header() {
    let mut state = SgbState::new();
    let mut packet = [0u8; 16];
    packet[0] = (0x14 << 3) | 0x03;

    let event = feed_packet(&mut state, packet).expect("expected PCT_TRN event");
    assert!(matches!(event, SgbEvent::PctTrn));
}

#[test]
fn decodes_mlt_req_mode() {
    let mut state = SgbState::new();
    let mut packet = [0u8; 16];
    packet[0] = (0x11 << 3) | 0x01;
    packet[1] = 0x01;

    let event = feed_packet(&mut state, packet).expect("expected MLT_REQ event");
    match event {
        SgbEvent::MltReq(mode) => assert_eq!(mode, 0x01),
        _ => panic!("unexpected event"),
    }
}

#[test]
fn multi_packet_continuation_consumed_not_misinterpreted() {
    let mut state = SgbState::new();

    let mut attr_blk_1 = [0u8; 16];
    attr_blk_1[0] = (0x04 << 3) | 0x02;
    attr_blk_1[1] = 0x01;
    let event = feed_packet(&mut state, attr_blk_1);
    assert!(
        event.is_none(),
        "First ATTR_BLK packet should buffer, not fire event"
    );

    let mut attr_blk_2 = [0u8; 16];
    attr_blk_2[0] = 0x01;
    let event = feed_packet(&mut state, attr_blk_2);
    assert!(
        matches!(event, Some(SgbEvent::AttrBlk(_))),
        "Second ATTR_BLK packet should produce AttrBlk event, not PAL01"
    );

    let mut pal01 = [0u8; 16];
    pal01[0] = 0x01;
    pal01[1] = 0xFF;
    pal01[2] = 0x7F;
    let event = feed_packet(&mut state, pal01).expect("expected PAL01 event after ATTR_BLK");
    assert!(matches!(event, SgbEvent::Pal01(..)));
}

#[test]
fn attr_blk_single_packet_returns_event() {
    let mut state = SgbState::new();
    let mut packet = [0u8; 16];
    packet[0] = (0x04 << 3) | 0x01;
    packet[1] = 0x01;
    packet[2] = 0x03;
    packet[3] = 0x05;
    packet[4] = 2;
    packet[5] = 2;
    packet[6] = 10;
    packet[7] = 10;

    let event = feed_packet(&mut state, packet).expect("expected AttrBlk event");
    match event {
        SgbEvent::AttrBlk(data) => {
            assert_eq!(data.len(), 16);
            assert_eq!(data[1], 1); // count
        }
        _ => panic!("expected AttrBlk, got {:?}", event),
    }
}

#[test]
fn attr_lin_single_packet_returns_event() {
    let mut state = SgbState::new();
    let mut packet = [0u8; 16];
    packet[0] = (0x05 << 3) | 0x01;
    packet[1] = 0x02;
    packet[2] = 0x03 | (1 << 5);
    packet[3] = 0x05 | (2 << 5) | 0x80;

    let event = feed_packet(&mut state, packet).expect("expected AttrLin event");
    assert!(matches!(event, SgbEvent::AttrLin(_)));
}

#[test]
fn attr_div_returns_event() {
    let mut state = SgbState::new();
    let mut packet = [0u8; 16];
    packet[0] = (0x06 << 3) | 0x01;
    packet[1] = 0x40 | (2 << 4) | (1 << 2);
    packet[2] = 5;

    let event = feed_packet(&mut state, packet).expect("expected AttrDiv event");
    match event {
        SgbEvent::AttrDiv(p) => {
            assert_eq!(p[1] & 0x40, 0x40);
            assert_eq!(p[2], 5);
        }
        _ => panic!("expected AttrDiv, got {:?}", event),
    }
}

#[test]
fn attr_chr_single_packet_returns_event() {
    let mut state = SgbState::new();
    let mut packet = [0u8; 16];
    packet[0] = (0x07 << 3) | 0x01;
    packet[1] = 0;
    packet[2] = 0;
    packet[3] = 8;
    packet[4] = 0;
    packet[5] = 0;
    packet[6] = 0b_00_01_10_11;
    packet[7] = 0b_11_10_01_00;

    let event = feed_packet(&mut state, packet).expect("expected AttrChr event");
    assert!(matches!(event, SgbEvent::AttrChr(_)));
}

#[test]
fn attr_set_parses_cancel_mask_flag() {
    let mut state = SgbState::new();
    let mut packet = [0u8; 16];
    packet[0] = (0x16 << 3) | 0x01;
    packet[1] = 0x43;

    let event = feed_packet(&mut state, packet).expect("expected AttrSet event");
    match event {
        SgbEvent::AttrSet(file_idx, cancel_mask) => {
            assert_eq!(file_idx, 3);
            assert!(cancel_mask);
        }
        _ => panic!("expected AttrSet, got {:?}", event),
    }
}
