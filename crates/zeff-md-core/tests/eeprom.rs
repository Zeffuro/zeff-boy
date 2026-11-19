#[path = "support/state_layout.rs"]
mod state_layout;

mod eeprom {
    pub mod acclaim;
    pub mod cases;
    pub mod fixture;
}

#[test]
fn serial_save_guest_protocol_and_restore() {
    eeprom::cases::guest_protocol_and_restore();
}

#[test]
fn serial_save_admission_persistence_and_rejection() {
    eeprom::cases::admission_persistence_and_rejection();
}

#[test]
fn separate_address_serial_guest_protocol_and_restore() {
    eeprom::acclaim::guest_protocol_and_restore();
}

#[test]
fn separate_address_serial_profiles_reset_and_rejection() {
    eeprom::acclaim::profiles_reset_and_rejection();
}

#[test]
fn header_address_serial_profile_and_restore() {
    eeprom::cases::header_address_profile_and_restore();
}
