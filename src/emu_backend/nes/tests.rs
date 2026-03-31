use super::map_host_to_nes_byte;

#[test]
fn nes_a_button() {
    assert_eq!(map_host_to_nes_byte(0x01, 0x00), 0x01);
}

#[test]
fn nes_b_button() {
    assert_eq!(map_host_to_nes_byte(0x02, 0x00), 0x02);
}

#[test]
fn nes_select_button() {
    assert_eq!(map_host_to_nes_byte(0x04, 0x00), 0x04);
}

#[test]
fn nes_start_button() {
    assert_eq!(map_host_to_nes_byte(0x08, 0x00), 0x08);
}

#[test]
fn nes_dpad_up() {
    assert_eq!(map_host_to_nes_byte(0x00, 0x04), 0x10);
}

#[test]
fn nes_dpad_down() {
    assert_eq!(map_host_to_nes_byte(0x00, 0x08), 0x20);
}

#[test]
fn nes_dpad_left() {
    assert_eq!(map_host_to_nes_byte(0x00, 0x02), 0x40);
}

#[test]
fn nes_dpad_right() {
    assert_eq!(map_host_to_nes_byte(0x00, 0x01), 0x80);
}

#[test]
fn nes_all_inputs() {
    assert_eq!(map_host_to_nes_byte(0x0F, 0x0F), 0xFF);
}

#[test]
fn nes_no_inputs() {
    assert_eq!(map_host_to_nes_byte(0x00, 0x00), 0x00);
}

#[test]
fn nes_combined_a_and_right() {
    assert_eq!(map_host_to_nes_byte(0x01, 0x01), 0x81);
}
