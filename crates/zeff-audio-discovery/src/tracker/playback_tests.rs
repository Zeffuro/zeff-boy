use super::tests::detect;
use crate::test_support::tracker::mod_fixture;

fn effect(bytes: &mut [u8], row: usize, channel: usize, effect: u8, value: u8) {
    let at = 1084 + (row * 4 + channel) * 4;
    bytes[at + 2] = (bytes[at + 2] & 0xf0) | effect;
    bytes[at + 3] = value;
}

#[test]
fn unsupported_mod_dialects_periods_and_effects_remain_extractable() {
    let original = mod_fixture();
    assert!(detect(&original)[0].mod_playback);
    for tag in [b"FLT4", b"4CHN"] {
        let mut bytes = original.clone();
        bytes[1080..1084].copy_from_slice(tag);
        assert!(!detect(&bytes)[0].mod_playback);
    }
    for period in [28u16, 55, 429, 4064, 4095] {
        let mut bytes = original.clone();
        bytes[1084] = (period >> 8) as u8;
        bytes[1085] = period as u8;
        assert!(!detect(&bytes)[0].mod_playback);
    }
    for (command, value) in [
        (0xe, 0),
        (0xe, 0x30),
        (0xe, 0x50),
        (0xe, 0x80),
        (0xe, 0xe1),
        (0xe, 0xf1),
        (0xf, 0),
        (0xd, 0x64),
        (0xd, 0x0a),
    ] {
        let mut bytes = original.clone();
        effect(&mut bytes, 1, 0, command, value);
        assert!(!detect(&bytes)[0].mod_playback, "{command:x}{value:02x}");
    }
    let mut bytes = original;
    effect(&mut bytes, 1, 0, 0xe, 1);
    assert!(detect(&bytes)[0].mod_playback);
    effect(&mut bytes, 2, 0, 0xb, 0);
    effect(&mut bytes, 2, 1, 0xd, 1);
    assert!(!detect(&bytes)[0].mod_playback);
}

#[test]
fn mod_pattern_loops_require_disjoint_bounded_blocks() {
    let mut bytes = mod_fixture();
    for (row, channel, value) in [
        (0, 2, 0x60),
        (14, 2, 0x61),
        (15, 2, 0x60),
        (29, 2, 0x61),
        (51, 2, 0x60),
        (57, 2, 0x63),
        (60, 1, 0x60),
        (62, 1, 0x63),
    ] {
        effect(&mut bytes, row, channel, 0xe, value);
    }
    assert!(detect(&bytes)[0].mod_playback);
    for (row, channel, command, value) in [
        (1, 0, 0xe, 0x60),
        (14, 2, 0, 0),
        (14, 1, 0xe, 0x61),
        (63, 0, 0xb, 0),
        (63, 0, 0xd, 0),
    ] {
        let mut malformed = bytes.clone();
        effect(&mut malformed, row, channel, command, value);
        assert!(!detect(&malformed)[0].mod_playback);
    }
    bytes[950] = 2;
    assert!(!detect(&bytes)[0].mod_playback);
}

#[test]
fn mod_playback_capability_is_revalidated_before_loading() {
    use super::verify_original;
    use std::sync::atomic::AtomicBool;
    let mut bytes = mod_fixture();
    let descriptor = detect(&bytes).remove(0);
    let cancel = AtomicBool::new(false);
    assert!(verify_original(&bytes, &descriptor, &cancel).is_ok());
    effect(&mut bytes, 1, 0, 0xe, 0xe1);
    assert!(verify_original(&bytes, &descriptor, &cancel).is_err());
    assert!(!detect(&bytes)[0].mod_playback);
}
