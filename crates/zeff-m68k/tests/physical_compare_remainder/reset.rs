use super::*;
use device::Device;
use zeff_m68k::timing::physical::{ExternalResetBoundary, ResetQualification};

pub(super) fn run(cpu: &mut PrefetchCpu, node: &value::Value) -> bool {
    let c = decode::reset_case(node.clone());
    let mut bus = Device::new(&c);
    let mut copied = cpu.clone();
    let mut copied_bus = bus.clone();
    let before = cpu.clone();
    let device = bus.clone();
    let boundary = ExternalResetBoundary {
        sampled_release: CpuClocks(c.initial.clock),
        qualification: ResetQualification::QualifiedSafeAssertionAndRelease,
    };
    let copied_result = copied.external_warm_reset(&mut copied_bus, boundary);
    let actual = cpu.external_warm_reset(&mut bus, boundary);
    let (tag, expected) = node.get("expected").get("typed_result").single();
    let outcome = match tag {
        "Err" => {
            expected.text() == "EntryFault::HostFrozen"
                && actual == Err(EntryFault::HostFrozen)
                && *cpu == before
                && bus == device
        }
        "Ok" => {
            let (kind, fields) = expected.single();
            kind == "EntryStep"
                && fields.get("kind").text() == "ExternalWarmReset"
                && fields.get("clocks_u64").number() == c.elapsed
                && matches!(actual, Ok(step) if step.kind == EntryKind::ExternalWarmReset && step.clocks == c.elapsed)
        }
        _ => panic!("reset result"),
    };
    outcome
        && copied_result == actual
        && copied == *cpu
        && copied_bus == bus
        && cpu.registers == c.expected.registers
        && cpu.queued_words() == c.expected.queue
        && cpu.clock().0 == c.expected.clock
        && cpu.physical_entry_state() == c.state
        && cpu.physical_inspection() == c.inspection
        && check::private_state(cpu, &c)
        && bus.words == c.final_memory.iter().copied().collect()
        && bus.events == c.events
        && bus.counts == c.counts
        && bus.clock == c.expected.clock
        && bus.before_entry == c.before_entry
        && controls::verify(&bus, &c)
}
