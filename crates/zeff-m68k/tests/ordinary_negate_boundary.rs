use zeff_m68k::Registers;
use zeff_m68k::bus::{AccessKind, BusAccess, ByteLanes, Direction, FunctionCode};
use zeff_m68k::timing::physical::{PhysicalEntryState, PhysicalInspection};
use zeff_m68k::timing::{ClockedBus, CpuClocks, PrefetchCpu, TimedStep, TimingFault};

#[path = "ordinary_negate/boundary.rs"]
mod boundary;
#[path = "ordinary_negate/bulk_retry.rs"]
mod bulk_retry;

#[test]
fn negate_forms_and_existing_from_sr() {
    assert_eq!(boundary::section21(), (318, 0));
}

#[test]
fn ordinary_trace_and_interrupt_boundaries() {
    assert_eq!(boundary::section22(), (1344, 0));
}

#[test]
fn existing_status_aliases_execute_correct_routes() {
    assert_eq!(boundary::section24(), (318, 0));
}
