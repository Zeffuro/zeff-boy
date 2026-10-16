use super::support::*;
use zeff_m68k::bus::{AccessKind, Direction, FunctionCode};
use zeff_m68k::timing::{PrefetchCpu, TimedOutcome, TimingFault};
use zeff_md_core::{Region, VdpScheduleError};

type Step = Result<TimedOutcome, TimingFault<CpuBusError>>;

fn assert_entry(cpu: &PrefetchCpu, bus: &CpuBus, accepted: u8, clocks: u64) {
    assert_eq!(cpu.registers.pc, if accepted == 4 { 0x200 } else { 0x300 });
    assert_eq!(cpu.registers.ssp, 0xffa);
    assert_eq!(
        cpu.registers.status_register(),
        0x2000 | (u16::from(accepted) << 8)
    );
    assert_eq!(cpu.queued_words(), Some([0x4e71, 0x7201]));
    assert_eq!(bus.clock.0, clocks);
    assert_eq!(bus.read_word(0xffe), 0x100);
    assert_eq!(bus.read_word(0xffc), 0);
    assert_eq!(bus.read_word(0xffa), 0x2000);
    assert_eq!(bus.state.now(), bus.registers.now());
}

pub fn actual_cpu_acknowledge_with(mut step: impl FnMut(&mut PrefetchCpu, &mut CpuBus) -> Step) {
    for (start, wait, cleared) in [
        (767010, 0, 6),
        (766970, 0, 4),
        (766970, 4, 6),
        (767021, 0, 6),
        (767010, 1, 6),
        (767010, 17, 6),
    ] {
        let mut bus = CpuBus::ready(start, true, false);
        bus.wait = wait;
        let mut cpu = cpu(&bus);
        let mut copied = (cpu.clone(), bus);
        let clocks = 44 + u64::from(wait);
        let expected = Ok(TimedOutcome::Interrupt {
            level: 4,
            vector: 28,
            clocks,
        });
        assert_eq!(step(&mut cpu, &mut bus), expected);
        assert_eq!(step(&mut copied.0, &mut copied.1), expected);
        assert_eq!(cpu, copied.0);
        assert_eq!(bus, copied.1);
        assert_entry(&cpu, &bus, 4, clocks);
        assert_eq!(bus.state.now(), tick(start + clocks * 7));
        let ack = bus
            .records
            .iter()
            .flatten()
            .find_map(|record| {
                if let Record::Ack(access, begin, end, cleared) = record {
                    Some((*access, *begin, *end, *cleared))
                } else {
                    None
                }
            })
            .unwrap();
        assert_eq!(ack.0.bus_address, 0xff_fff8);
        assert_eq!(ack.0.logical_address, 0xffff_fff8);
        assert_eq!(ack.0.function_code, FunctionCode::CpuSpace);
        assert_eq!(ack.0.kind, AccessKind::InterruptAcknowledge);
        assert_eq!(
            (ack.1, ack.2, ack.3),
            (start + 70, start + (14 + u64::from(wait)) * 7, cleared)
        );
        assert_eq!(bus.state.horizontal_pending(), cleared != 4);
        assert_eq!(bus.state.vertical_pending(), cleared == 4);
        assert_eq!(
            bus.state.interrupt_level(&bus.registers),
            Ok(if cleared == 4 { 6 } else { 4 })
        );
    }
}
pub fn actual_cpu_acknowledge() {
    actual_cpu_acknowledge_with(|cpu, bus| cpu.step_boundary(bus));
}

pub fn actual_cpu_publications_with(mut step: impl FnMut(&mut PrefetchCpu, &mut CpuBus) -> Step) {
    for (initial, value, accepted, cleared, output) in
        [(0x44, 0x64, 4, 6, 4), (0x64, 0x44, 6, 4, 0)]
    {
        let mut bus = CpuBus::ready(767061, true, true);
        let (mut state, mut registers) = known(1000, 255, true, true);
        publish(&mut registers, 1, initial);
        state
            .advance_to(registers.now(), Region::NtscUs, &registers)
            .unwrap();
        drain(&mut state, &mut registers, 767061);
        bus.state = state;
        bus.registers = registers;
        let write = bus.registers.queue_register(1, value).unwrap();
        assert_eq!(write.publish_at, tick(767080));
        let mut cpu = cpu(&bus);
        assert_eq!(cpu.interrupt_input().level(), accepted);
        let mut copied = (cpu.clone(), bus);
        let expected = Ok(TimedOutcome::Interrupt {
            level: accepted,
            vector: 24 + accepted,
            clocks: 44,
        });
        assert_eq!(step(&mut cpu, &mut bus), expected);
        assert_eq!(step(&mut copied.0, &mut copied.1), expected);
        assert_eq!(cpu, copied.0);
        assert_eq!(bus, copied.1);
        assert_entry(&cpu, &bus, accepted, 44);
        assert_eq!(bus.events[0], Some(zeff_md_core::VdpEvent::Register(write)));
        assert_eq!(bus.registers.pending(), None);
        assert_eq!(bus.state.interrupt_level(&bus.registers), Ok(output));
        assert_eq!(bus.state.status_bits(), if cleared == 6 { 0 } else { 0x80 });
        let ack = bus
            .records
            .iter()
            .flatten()
            .find(|record| matches!(record, Record::Ack(..)))
            .unwrap();
        assert!(
            matches!(ack, Record::Ack(access, 767131, 767159, current) if *current == cleared && access.bus_address == (0xff_fff0 | (u32::from(accepted) << 1)))
        );
    }
}
pub fn actual_cpu_publications() {
    actual_cpu_publications_with(|cpu, bus| cpu.step_boundary(bus));
}

pub fn actual_cpu_faults_with(mut step: impl FnMut(&mut PrefetchCpu, &mut CpuBus) -> Step) {
    for trial in 0..3 {
        let start = if trial == 2 { 766992 } else { 767010 };
        let mut bus = CpuBus::ready(start, true, false);
        bus.fail_ack = trial == 0;
        bus.unsupported_ack = trial == 1;
        let mut cpu = cpu(&bus);
        let result = step(&mut cpu, &mut bus);
        if trial == 1 {
            assert_eq!(result, Err(TimingFault::InterruptAcknowledgeUnsupported));
        } else {
            let source = if trial == 0 {
                CpuBusError::Injected
            } else {
                CpuBusError::Schedule(VdpScheduleError::CoincidentAcknowledge(tick(767090)))
            };
            assert!(
                matches!(result, Err(TimingFault::Bus { access, direction: Direction::Read, source: actual }) if actual == source && access.bus_address == 0xff_fff8)
            );
        }
        assert_eq!(bus.state.now(), tick(start + 70));
        assert_eq!(bus.state.now(), bus.registers.now());
        assert_eq!(bus.clock.0, 10);
        assert_eq!(bus.read_word(0xffe), 0x100);
        assert_eq!(bus.read_word(0xffa), 0);
        assert_eq!(bus.state.status_bits(), 0);
        assert!(bus.state.horizontal_pending());
        assert_eq!(cpu.registers.ssp, 0x1000);
        assert_eq!(cpu.registers.pc, 0x100);
        let frozen = (cpu.clone(), bus);
        assert_eq!(step(&mut cpu, &mut bus), Err(TimingFault::Frozen));
        assert_eq!(cpu, frozen.0);
        assert_eq!(bus, frozen.1);
    }
}
pub fn actual_cpu_faults() {
    actual_cpu_faults_with(|cpu, bus| cpu.step_boundary(bus));
}
