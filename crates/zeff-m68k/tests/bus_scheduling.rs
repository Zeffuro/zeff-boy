#[path = "bus_scheduling/cases.rs"]
mod cases;

#[test]
fn held_long_write_advances_devices_without_repeating_high_word() {
    cases::held_long_write_advances_devices_without_repeating_high_word();
}

#[test]
fn held_read_preserves_prior_write_and_samples_each_half_once() {
    cases::held_read_preserves_prior_write_and_samples_each_half_once();
}

#[test]
fn actual_cpu_byte_writes_preserve_the_other_lane_and_ram_aliases() {
    cases::actual_cpu_byte_writes_preserve_the_other_lane_and_ram_aliases();
}
