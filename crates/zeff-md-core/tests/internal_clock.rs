#[path = "internal_clock/machine.rs"]
mod machine;

#[test]
fn cpu_internal_clock_fields_and_frontend_restore_preserve_future_output() {
    assert_ne!(machine::run(), 0);
}
