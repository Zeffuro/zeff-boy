#[path = "ordinary_clear_not_entry/binding.rs"]
mod binding;
#[path = "ordinary_clear_not_entry/device.rs"]
mod device;
#[path = "ordinary_clear_not_entry/entry_axes.rs"]
mod entry_axes;
#[path = "ordinary_clear_not_entry/entry_binding.rs"]
mod entry_binding;
#[path = "ordinary_clear_not_entry/entry_check.rs"]
mod entry_check;
#[path = "ordinary_clear_not_entry/entry_engine.rs"]
mod entry_engine;
#[path = "ordinary_clear_not_primary/fixed.rs"]
mod fixed;
#[path = "ordinary_clear_not_entry/literal.rs"]
mod literal;
#[path = "ordinary_clear_not_entry/oracle.rs"]
mod oracle;
#[path = "ordinary_clear_not_entry/seed.rs"]
mod seed;

#[test]
fn irq_and_trace_entry_whole_states() {
    assert_eq!(entry_check::all(true), (297216, 0));
}

#[test]
fn source_literals_bind_whole_states() {
    assert_eq!(entry_check::literals(true), (448, 0));
}
