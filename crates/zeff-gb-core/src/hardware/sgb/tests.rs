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
    packet[0] = (0x00 << 3) | 0x01;
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

