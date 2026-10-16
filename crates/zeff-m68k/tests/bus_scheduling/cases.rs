use zeff_emu_common::time::{MachineTiming, MasterTicks};
use zeff_m68k::Cpu;
use zeff_m68k::bus::{AccessKind, ByteLanes, Direction, FunctionCode};
use zeff_m68k::timing::{PrefetchCpu, TimedOutcome};
#[path = "device.rs"]
mod device;
use device::Device;

fn prepared(program: &[u16], address: u32) -> (PrefetchCpu, Device) {
    let mut bus = Device::new(program);
    let mut architectural = Cpu::new();
    architectural.reset(&mut bus).unwrap();
    assert_eq!(
        (architectural.registers.ssp, architectural.registers.pc),
        (0x00ff_fffc, 0x100)
    );
    assert_eq!(
        bus.reset_reads
            .iter()
            .map(|access| access.bus_address)
            .collect::<Vec<_>>(),
        [0, 2, 4, 6]
    );
    assert!(
        bus.reset_reads
            .iter()
            .all(|access| access.kind == AccessKind::ResetVector
                && access.function_code == FunctionCode::SupervisorProgram)
    );
    assert_eq!(bus.ticks, MasterTicks::ZERO);
    let mut cpu = PrefetchCpu::default();
    cpu.registers = architectural.registers;
    cpu.registers.a[0] = address;
    cpu.prime(&mut bus).unwrap();
    assert_eq!((cpu.clock().0, bus.cpu_clock), (8, 8));
    assert_eq!(bus.ticks, MasterTicks::new(56));
    (cpu, bus)
}

fn step(cpu: &mut PrefetchCpu, bus: &mut Device) -> u64 {
    let TimedOutcome::Instruction(result) = cpu.step_boundary(bus).unwrap() else {
        panic!("instruction")
    };
    assert_eq!(cpu.clock().0, bus.cpu_clock);
    result.clocks
}

pub fn held_long_write_advances_devices_without_repeating_high_word() {
    let (mut cpu, mut bus) = prepared(&[0x70ff, 0x2080, 0x4e71, 0x4e71], 0x01ff_0000);
    assert_eq!(step(&mut cpu, &mut bus), 4);
    bus.hold = Some((0xff_0002, Direction::Write, 20));
    let (mut copied_cpu, mut copied_bus) = (cpu.clone(), bus.clone());
    let (mut control_cpu, mut control_bus) = (cpu.clone(), bus.clone());
    control_bus.hold = None;
    assert_eq!(step(&mut cpu, &mut bus), 16);
    assert_eq!(step(&mut copied_cpu, &mut copied_bus), 16);
    assert_eq!(step(&mut control_cpu, &mut control_bus), 12);
    assert_eq!(cpu, copied_cpu);
    assert_eq!(bus, copied_bus);
    assert_eq!(cpu.registers, control_cpu.registers);
    assert_eq!(cpu.queued_words(), control_cpu.queued_words());
    assert_eq!(bus.ram, control_bus.ram);
    assert_eq!(bus.timing_snapshot().now(), MasterTicks::new(196));
    assert_eq!(control_bus.ticks, MasterTicks::new(168));
    assert_eq!((bus.device_cycles, bus.device_phase), (13, 1));
    assert_eq!(
        bus.held_ticks,
        [119, 126, 133, 140].map(|ticks| (MasterTicks::new(ticks), [255, 255, 0, 0]))
    );
    let writes: Vec<_> = bus
        .transfers
        .iter()
        .filter(|t| t.direction == Direction::Write)
        .collect();
    assert_eq!(writes.len(), 2);
    assert_eq!(
        (writes[0].start, writes[0].end, writes[0].waited),
        (12, 16, 0)
    );
    assert_eq!(
        (writes[1].start, writes[1].end, writes[1].waited),
        (16, 24, 4)
    );
    assert_eq!(
        writes
            .iter()
            .map(|t| t.access.logical_address)
            .collect::<Vec<_>>(),
        [0x01ff_0000, 0x01ff_0002]
    );
    assert_eq!(
        writes
            .iter()
            .map(|t| t.access.bus_address)
            .collect::<Vec<_>>(),
        [0xff_0000, 0xff_0002]
    );
    assert!(
        writes
            .iter()
            .all(|t| t.access.lanes == ByteLanes::Both && t.value == 0xffff)
    );
    assert_eq!(&bus.ram[..4], &[255; 4]);
}

pub fn held_read_preserves_prior_write_and_samples_each_half_once() {
    let (mut cpu, mut bus) = prepared(&[0x70ff, 0x2080, 0x2210, 0x4e71, 0x4e71], 0x01ff_0000);
    step(&mut cpu, &mut bus);
    step(&mut cpu, &mut bus);
    assert_eq!((cpu.clock().0, bus.cpu_clock), (24, 24));
    assert_eq!(&bus.ram[..4], &[255; 4]);
    bus.hold = Some((0xff_0002, Direction::Read, 32));
    let (mut copied_cpu, mut copied_bus) = (cpu.clone(), bus.clone());
    let (mut control_cpu, mut control_bus) = (cpu.clone(), bus.clone());
    control_bus.hold = None;
    assert_eq!(step(&mut cpu, &mut bus), 16);
    assert_eq!(step(&mut copied_cpu, &mut copied_bus), 16);
    assert_eq!(step(&mut control_cpu, &mut control_bus), 12);
    assert_eq!(cpu, copied_cpu);
    assert_eq!(bus, copied_bus);
    assert_eq!(cpu.registers, control_cpu.registers);
    assert_eq!(cpu.queued_words(), control_cpu.queued_words());
    assert_eq!(bus.ram, control_bus.ram);
    assert_eq!(cpu.registers.d[1], u32::MAX);
    assert_eq!((bus.device_cycles, bus.device_phase), (18, 10));
    assert_eq!(bus.ticks, MasterTicks::new(280));
    assert_eq!(
        bus.held_ticks,
        [203, 210, 217, 224].map(|ticks| (MasterTicks::new(ticks), [255; 4]))
    );
    let reads: Vec<_> = bus
        .transfers
        .iter()
        .filter(|t| t.access.kind == AccessKind::Data && t.direction == Direction::Read)
        .collect();
    assert_eq!(reads.len(), 2);
    assert_eq!((reads[0].start, reads[0].end), (24, 28));
    assert_eq!((reads[1].start, reads[1].end, reads[1].waited), (28, 36, 4));
    assert_eq!(
        bus.transfers
            .iter()
            .filter(|t| t.direction == Direction::Write)
            .count(),
        2
    );
}

pub fn actual_cpu_byte_writes_preserve_the_other_lane_and_ram_aliases() {
    for (address, expected, lane) in [
        (0x01e0_0000, [0x55, 0x34], ByteLanes::Upper),
        (0x01ff_0001, [0x12, 0x55], ByteLanes::Lower),
    ] {
        let (mut cpu, mut bus) = prepared(&[0x7055, 0x1080, 0x4e71, 0x4e71], address);
        bus.ram[..2].copy_from_slice(&[0x12, 0x34]);
        step(&mut cpu, &mut bus);
        step(&mut cpu, &mut bus);
        assert_eq!(&bus.ram[..2], &expected);
        let writes: Vec<_> = bus
            .transfers
            .iter()
            .filter(|t| t.direction == Direction::Write)
            .collect();
        assert_eq!(writes.len(), 1);
        assert_eq!(writes[0].access.lanes, lane);
        assert_eq!(writes[0].access.logical_address, address);
    }
}
