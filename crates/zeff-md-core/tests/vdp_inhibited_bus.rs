#[path = "vdp_inhibited_bus/cases.rs"]
mod cases;

#[test]
fn ignored_write_main_bus_and_restored_continuations_match() {
    cases::main_bus_ignored_write_restore();
}
