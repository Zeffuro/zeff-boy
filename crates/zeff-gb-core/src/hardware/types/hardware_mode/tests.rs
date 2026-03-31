use super::{HardwareMode, HardwareModePreference};

#[test]
fn auto_prefers_cgb_for_cgb_roms() {
    let mode = HardwareModePreference::Auto.resolve(true, true, 0x33);
    assert_eq!(mode, HardwareMode::CGBNormal);
}

#[test]
fn auto_uses_sgb_when_header_matches() {
    let mode = HardwareModePreference::Auto.resolve(false, true, 0x33);
    assert_eq!(mode, HardwareMode::SGB1);
}

#[test]
fn auto_falls_back_to_dmg_when_not_sgb() {
    let mode = HardwareModePreference::Auto.resolve(false, true, 0x01);
    assert_eq!(mode, HardwareMode::DMG);
}
