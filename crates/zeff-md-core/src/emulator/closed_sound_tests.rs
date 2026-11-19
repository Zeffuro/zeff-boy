use super::*;
use crate::constants::{SOUND_BUS_REQUEST, SOUND_RESET, VDP_STATUS_FLOAT_MASK};
use crate::hardware::sound::schedule::PendingSound;
use zeff_m68k::bus::{AccessKind, BusAccess, ByteLanes, Direction, FunctionCode};
use zeff_m68k::timing::{ClockedBus, CycleResult};

fn machine(program: &[u16], concurrent: bool, reset: bool, region: Region) -> Emulator {
    let mut image = vec![0; 0x200];
    image[..8].copy_from_slice(&[0, 0xff, 0xff, 0xfc, 0, 0, 1, 0]);
    for (index, word) in program.iter().enumerate() {
        image[0x100 + index * 2..0x102 + index * 2].copy_from_slice(&word.to_be_bytes());
    }
    let mut result = Emulator::new(
        image,
        LoadConfig {
            region,
            concurrent_sound: concurrent,
        },
    )
    .unwrap();
    result.bus.sound.ram.fill(0xde);
    result.bus.sound.ram[..16].fill(0);
    result.bus.sound.bank = 0x155;
    result.bus.sound.set_reset(reset);
    result
}

fn access(address: u32, kind: AccessKind, lanes: ByteLanes) -> BusAccess {
    BusAccess {
        logical_address: address + u32::from(lanes == ByteLanes::Lower),
        bus_address: address,
        function_code: FunctionCode::SupervisorData,
        kind,
        lanes,
    }
}

fn read(machine: &mut Emulator, access: BusAccess) -> CycleResult<u16, BusError> {
    let clock = machine.bus.cpu_clock;
    if machine.concurrent_sound {
        ScheduledBus::new(
            &mut machine.bus,
            &mut machine.sound_cpu,
            &mut machine.sound_schedule,
        )
        .read_cycle(access, clock)
    } else {
        machine.bus.read_cycle(access, clock)
    }
}

fn write(machine: &mut Emulator, access: BusAccess, value: u16) -> CycleResult<(), BusError> {
    let clock = machine.bus.cpu_clock;
    if machine.concurrent_sound {
        ScheduledBus::new(
            &mut machine.bus,
            &mut machine.sound_cpu,
            &mut machine.sound_schedule,
        )
        .write_cycle(access, value, clock)
    } else {
        machine.bus.write_cycle(access, value, clock)
    }
}

fn seed(machine: &mut Emulator) {
    machine.bus.ram[0x100..0x102].copy_from_slice(&[0x12, 0x34]);
    assert_eq!(
        read(
            machine,
            access(0xff0100, AccessKind::Prefetch, ByteLanes::Both)
        )
        .result,
        Ok(0x1234)
    );
    machine.bus.ram[0x100..0x102].copy_from_slice(&[0xee, 0xef]);
    assert_eq!(machine.bus.prefetch, Prefetch::Full(0x1234));
}

fn failure(address: u32) -> Result<u16, BusError> {
    Err(BusError::UnsupportedAccess {
        address,
        direction: Direction::Read,
    })
}

fn same_elapsed(actual: &Emulator, expected: &Emulator) {
    let mut normalized = actual.clone();
    normalized.bus.prefetch = expected.bus.prefetch;
    assert_eq!(&normalized, expected);
}

#[cfg_attr(test, test)]
pub(crate) fn closed_read_lanes_repeat_and_never_sample_devices() {
    for region in [Region::NtscUs, Region::NtscJapan, Region::Pal] {
        for concurrent in [false, true] {
            for reset in [false, true] {
                for address in [
                    0xa00008, 0xa0000a, 0xa00044, 0xa04000, 0xa06000, 0xa07f10, 0xa07f20, 0xa08008,
                    0xa0fffe,
                ] {
                    for (lanes, value) in [
                        (ByteLanes::Both, 0x1234),
                        (ByteLanes::Upper, 0x1200),
                        (ByteLanes::Lower, 0x34),
                    ] {
                        for code in [FunctionCode::SupervisorData, FunctionCode::UserData] {
                            let mut actual = machine(&[0x4e71; 4], concurrent, reset, region);
                            seed(&mut actual);
                            let mut control = actual.clone();
                            let mut request = access(address, AccessKind::Data, lanes);
                            request.function_code = code;
                            for _ in 0..2 {
                                let now = actual.bus.now.get();
                                let result = read(&mut actual, request);
                                assert_eq!(result.result, Ok(value));
                                assert_eq!(result.wait_clocks, 0);
                                assert_eq!(actual.bus.now.get(), now + 28);
                                assert_eq!(actual.bus.prefetch, Prefetch::Full(0x1234));
                                assert_eq!(
                                    read(&mut control, access(0xff0100, AccessKind::Data, lanes))
                                        .result,
                                    Ok(0xeeef)
                                );
                                same_elapsed(&actual, &control);
                            }
                        }
                    }
                }
            }
        }
    }
}

struct Trace<'a> {
    bus: &'a mut dyn ClockedBus<Error = BusError>,
    reads: Vec<(AccessKind, u32, ByteLanes, u16, u64)>,
    writes: Vec<(u32, ByteLanes, u16)>,
}

impl ClockedBus for Trace<'_> {
    type Error = BusError;

    fn read_cycle(&mut self, access: BusAccess, start: CpuClocks) -> CycleResult<u16, BusError> {
        let result = self.bus.read_cycle(access, start);
        assert_eq!(result.wait_clocks, 0);
        if let Ok(value) = result.result {
            self.reads.push((
                access.kind,
                access.bus_address,
                access.lanes,
                value,
                start.0,
            ));
        }
        result
    }

    fn write_cycle(
        &mut self,
        access: BusAccess,
        value: u16,
        start: CpuClocks,
    ) -> CycleResult<(), BusError> {
        self.writes.push((access.bus_address, access.lanes, value));
        let result = self.bus.write_cycle(access, value, start);
        assert_eq!(result.wait_clocks, 0);
        result
    }

    fn idle(&mut self, start: CpuClocks, clocks: u32) {
        self.bus.idle(start, clocks);
    }
}

#[cfg_attr(test, test)]
pub(crate) fn real_tst_long_clr_and_move_sample_asymmetric_prefetch() {
    for concurrent in [false, true] {
        for reset in [false, true] {
            for (opcode, address, expected, register) in [
                (
                    0x4ab9,
                    0x0008,
                    vec![
                        (0xa00008, ByteLanes::Both, 0x1234),
                        (0xa0000a, ByteLanes::Both, 0x1234),
                    ],
                    None,
                ),
                (
                    0x4239,
                    0x0044,
                    vec![(0xa00044, ByteLanes::Upper, 0x1200)],
                    None,
                ),
                (
                    0x1039,
                    0x0045,
                    vec![(0xa00044, ByteLanes::Lower, 0x34)],
                    Some(0x34),
                ),
                (
                    0x3039,
                    0x0044,
                    vec![(0xa00044, ByteLanes::Both, 0x1234)],
                    Some(0x1234),
                ),
            ] {
                let mut machine = machine(
                    &[opcode, 0x00a0, address, 0x1234, 0x4e71, 0x4e71],
                    concurrent,
                    reset,
                    Region::NtscUs,
                );
                let sound_ram = machine.bus.sound.ram.clone();
                let main_ram = machine.bus.ram.clone();
                let mut scheduled = ScheduledBus::new(
                    &mut machine.bus,
                    &mut machine.sound_cpu,
                    &mut machine.sound_schedule,
                );
                let inner: &mut dyn ClockedBus<Error = BusError> = if concurrent {
                    &mut scheduled
                } else {
                    scheduled.bus
                };
                let mut trace = Trace {
                    bus: inner,
                    reads: Vec::new(),
                    writes: Vec::new(),
                };
                machine.cpu.step(&mut trace).unwrap();
                let observed: Vec<_> = trace
                    .reads
                    .iter()
                    .filter(|row| row.0 == AccessKind::Data)
                    .map(|row| (row.1, row.2, row.3))
                    .collect();
                assert_eq!(observed, expected);
                let data = trace
                    .reads
                    .iter()
                    .position(|row| row.0 == AccessKind::Data)
                    .unwrap();
                assert!(
                    trace.reads[..data]
                        .iter()
                        .any(|row| row.0 == AccessKind::Prefetch && row.3 == 0x1234)
                );
                if opcode == 0x4ab9 {
                    let times: Vec<_> = trace
                        .reads
                        .iter()
                        .filter(|row| row.0 == AccessKind::Data)
                        .map(|row| row.4)
                        .collect();
                    assert_eq!(times[1] - times[0], 4);
                }
                assert_eq!(
                    trace.writes,
                    if opcode == 0x4239 {
                        vec![(0xa00044, ByteLanes::Upper, 0)]
                    } else {
                        vec![]
                    }
                );
                if let Some(value) = register {
                    assert_eq!(machine.cpu.registers.d[0], value);
                }
                assert_eq!(machine.bus.sound.ram, sound_ram);
                assert_eq!(machine.bus.ram, main_ram);
                assert_eq!(machine.bus.sound.bank, 0x155);
                assert!(!machine.bus.sound.requested && !machine.bus.sound.granted);
            }
        }
    }
}

fn pending(program: &[u8], start: u64) -> Emulator {
    let mut result = machine(&[0x4e71; 8], true, false, Region::NtscUs);
    result.bus.sound.bank = 0x1ff;
    result.bus.sound.ram[..program.len()].copy_from_slice(program);
    result.bus.sound.ram[0x100] = 0x5a;
    result.sound_schedule.pending = Some(
        PendingSound::prepare(
            &result.sound_cpu,
            &result.bus.sound,
            &result.bus.cartridge,
            start,
        )
        .unwrap(),
    );
    result
}

fn position(machine: &mut Emulator, at: u64) {
    machine
        .bus
        .advance_machine(at - machine.bus.now.get())
        .unwrap();
    machine.bus.prefetch = Prefetch::Full(0x1234);
}

#[cfg_attr(test, test)]
pub(crate) fn local_publications_before_tie_after_preserve_observation() {
    for phase in 0..15 {
        for program in [[0x3a, 0, 1], [0x32, 0, 1]] {
            for offset in [-1i64, 0, 1, 27] {
                let mut actual = pending(&program, 300 + phase);
                let deadline = actual.sound_schedule.pending.as_ref().unwrap().deadline;
                let end = deadline.checked_add_signed(offset).unwrap();
                position(&mut actual, end - 28);
                let mut control = actual.clone();
                let result = read(
                    &mut actual,
                    access(0xa00044, AccessKind::Data, ByteLanes::Upper),
                );
                assert_eq!(result.result, Ok(0x1200));
                assert_eq!(result.wait_clocks, 0);
                assert_eq!(actual.bus.now.get(), end);
                assert_eq!(actual.bus.prefetch, Prefetch::Full(0x1234));
                read(
                    &mut control,
                    access(0xff0200, AccessKind::Data, ByteLanes::Upper),
                )
                .result
                .unwrap();
                same_elapsed(&actual, &control);
                assert_eq!(
                    actual.bus.sound.ram[0x100],
                    if program[0] == 0x32 && offset >= 0 {
                        0
                    } else {
                        0x5a
                    }
                );
                if offset < 0 {
                    assert_eq!(actual.sound_cpu.cycles(), 0);
                } else {
                    assert!(actual.sound_cpu.cycles() >= 13);
                }
            }
        }
    }
}

#[cfg_attr(test, test)]
pub(crate) fn banked_driver_that_publishes_and_vanishes_still_invalidates() {
    for offset in [-1i64, 0, 1] {
        let mut actual = pending(&[0x32, 0, 0x80, 0], 300);
        actual.bus.sound.bank = 0x1ff;
        actual.sound_schedule.pending = Some(
            PendingSound::prepare(
                &actual.sound_cpu,
                &actual.bus.sound,
                &actual.bus.cartridge,
                300,
            )
            .unwrap(),
        );
        let pending = actual.sound_schedule.pending.as_ref().unwrap();
        assert!(pending.any_bank_access && pending.bank_access.is_none());
        let deadline = pending.deadline;
        position(
            &mut actual,
            deadline.checked_add_signed(offset).unwrap() - 28,
        );
        actual.bus.ram[0x8000] = 0xde;
        let result = read(
            &mut actual,
            access(0xa00008, AccessKind::Data, ByteLanes::Both),
        );
        assert_eq!(result.result, failure(0xa00008));
        assert_eq!(result.wait_clocks, 0);
        assert_eq!(actual.bus.prefetch, Prefetch::Empty);
        if offset >= 0 {
            assert_eq!(actual.bus.ram[0x8000], 0);
            if offset > 0 {
                let final_pending = actual.sound_schedule.pending.as_ref().unwrap();
                assert!(!final_pending.any_bank_access && final_pending.bank_access.is_none());
            }
        } else {
            assert_eq!(actual.bus.ram[0x8000], 0xde);
        }
    }
    for fault in [false, true] {
        let mut actual = pending(&[0x3a, 0, 1], 300);
        position(&mut actual, 350);
        if fault {
            actual.sound_schedule.fault = Some(SoundFault::ClockOverflow);
        } else {
            actual
                .sound_schedule
                .pending
                .as_mut()
                .unwrap()
                .any_bank_access = true;
        }
        assert_eq!(
            read(
                &mut actual,
                access(0xa00008, AccessKind::Data, ByteLanes::Both)
            )
            .result,
            failure(0xa00008)
        );
        assert_eq!(actual.bus.prefetch, Prefetch::Empty);
    }
}

#[cfg_attr(test, test)]
pub(crate) fn waits_and_failed_clocks_do_not_restore_entry_history() {
    let mut reserved = pending(&[0x32, 0, 0x80], 300);
    reserved
        .sound_schedule
        .pending
        .as_mut()
        .unwrap()
        .bank_access = Some(crate::hardware::sound::BankAccess {
        address: 0xff8000,
        direction: Direction::Write,
        value: 0,
    });
    position(&mut reserved, 470);
    let result = read(
        &mut reserved,
        access(0xa00008, AccessKind::Data, ByteLanes::Both),
    );
    assert!(result.wait_clocks > 0);
    assert_eq!(result.result, failure(0xa00008));
    assert_eq!(reserved.bus.prefetch, Prefetch::Empty);

    for concurrent in [false, true] {
        {
            let mut machine = machine(&[0x4e71; 8], concurrent, true, Region::NtscUs);
            for (index, value) in [
                (15, 2),
                (19, 17),
                (20, 0),
                (21, 0),
                (22, 0),
                (23, 0),
                (1, 0x14),
            ] {
                machine.bus.advance_machine(1).unwrap();
                let write = machine
                    .bus
                    .vdp
                    .queue_register(index, value, Region::NtscUs)
                    .unwrap();
                machine
                    .bus
                    .advance_machine(write.publish_at.get() - machine.bus.now.get())
                    .unwrap();
            }
            seed(&mut machine);
            machine
                .bus
                .vdp
                .write_control(0x4000, Region::NtscUs)
                .unwrap();
            machine.bus.vdp.write_control(0x80, Region::NtscUs).unwrap();
            let result = read(
                &mut machine,
                access(0xa00008, AccessKind::Data, ByteLanes::Both),
            );
            assert!(result.wait_clocks > 0);
            assert_eq!(result.result, failure(0xa00008));
            assert_eq!(machine.bus.prefetch, Prefetch::Empty);
        }

        let mut machine = machine(&[0x4e71; 4], concurrent, true, Region::NtscUs);
        seed(&mut machine);
        let saved = machine.clone();
        let start = CpuClocks(machine.bus.cpu_clock.0 + 1);
        let request = access(0xa00008, AccessKind::Data, ByteLanes::Both);
        let result = if concurrent {
            ScheduledBus::new(
                &mut machine.bus,
                &mut machine.sound_cpu,
                &mut machine.sound_schedule,
            )
            .read_cycle(request, start)
        } else {
            machine.bus.read_cycle(request, start)
        };
        assert!(matches!(result.result, Err(BusError::ClockMismatch { .. })));
        assert_eq!(machine, saved);
        machine.bus.now = MasterTicks::new(u64::MAX - 1);
        let saved = machine.clone();
        assert_eq!(
            read(&mut machine, request).result,
            Err(BusError::ClockOverflow)
        );
        assert_eq!(machine, saved);
    }
}

#[cfg_attr(test, test)]
pub(crate) fn observer_keeps_complete_interval_evidence_and_scheduler_results() {
    use crate::hardware::sound::schedule::SoundInterval;
    for banked in [false, true] {
        for requested in [false, true] {
            let mut actual = pending(
                &[
                    if banked { 0x32 } else { 0x3a },
                    0,
                    if banked { 0x80 } else { 1 },
                    0,
                ],
                300,
            );
            actual.bus.sound.request(requested);
            actual.bus.ram[0x8000] = 0xde;
            let mut ordinary = actual.clone();
            let local = actual.sound_schedule.advance_observed(
                &mut actual.sound_cpu,
                &mut actual.bus.sound,
                &mut actual.bus.cartridge,
                &mut actual.bus.ram,
                SoundInterval::new(300..700, &mut actual.bus.audio),
            );
            ordinary.sound_schedule.advance(
                &mut ordinary.sound_cpu,
                &mut ordinary.bus.sound,
                &mut ordinary.bus.cartridge,
                &mut ordinary.bus.ram,
                SoundInterval::new(300..700, &mut ordinary.bus.audio),
            );
            assert_eq!(actual, ordinary);
            assert_eq!(local, !banked);
            assert_eq!(actual.bus.ram[0x8000], if banked { 0 } else { 0xde });
            if requested {
                assert!(actual.sound_schedule.pending.is_none());
                assert!(actual.bus.sound.granted);
            } else {
                let last = actual.sound_schedule.pending.as_ref().unwrap();
                assert!(!last.any_bank_access && last.bank_access.is_none());
            }
        }
    }
    let mut granted = pending(&[0x32, 0, 1], 300);
    let deadline = granted.sound_schedule.pending.as_ref().unwrap().deadline;
    position(&mut granted, deadline - 28);
    granted.bus.sound.request(true);
    let result = read(
        &mut granted,
        access(0xa00044, AccessKind::Data, ByteLanes::Both),
    );
    assert_eq!(result.result, Ok(0xdede));
    assert_eq!(result.wait_clocks, 0);
    assert!(granted.bus.sound.granted);
    assert_eq!(granted.bus.prefetch, Prefetch::Empty);
    assert_eq!(granted.bus.sound.ram[0x100], 0);
}

pub(crate) mod history;

pub(crate) mod reset;
