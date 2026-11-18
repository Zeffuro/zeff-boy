use super::*;
use std::sync::Arc;
use zeff_emu_common::save_state::{StateReader, StateWriter};
use zeff_m68k::Registers;
use zeff_m68k::bus::FunctionCode;
use zeff_m68k::timing::PrefetchCpu;
use zeff_m68k::timing::physical::{EntryFault, ExternalResetBoundary, ResetQualification};

#[derive(Clone, Debug, PartialEq, Eq)]
enum Event {
    Idle(CpuClocks, u32),
    Read(BusAccess, CpuClocks, EntryReport<u16, ResetBusError>),
}

struct Recorded<'a> {
    entry: ResetEntry<'a>,
    events: Vec<Event>,
}

impl EntryBus for Recorded<'_> {
    type Error = ResetBusError;
    fn read_entry(&mut self, access: BusAccess, start: CpuClocks) -> EntryReport<u16, Self::Error> {
        let report = self.entry.read_entry(access, start);
        self.events.push(Event::Read(access, start, report.clone()));
        report
    }
    fn write_entry(
        &mut self,
        access: BusAccess,
        value: u16,
        start: CpuClocks,
    ) -> EntryReport<(), Self::Error> {
        self.entry.write_entry(access, value, start)
    }
    fn idle_entry(&mut self, start: CpuClocks, clocks: u32) {
        self.events.push(Event::Idle(start, clocks));
        self.entry.idle_entry(start, clocks);
    }
}

fn fixture(region: Region, supervisor: bool) -> (PrefetchCpu, Bus) {
    let mut image = vec![0; 0x200];
    image[..8].copy_from_slice(&[0, 0xff, 0x10, 0, 0, 0, 1, 0]);
    image[0x100..0x108].copy_from_slice(&[0x4e, 0x71, 0x4e, 0x71, 0x4e, 0x71, 0x4e, 0x71]);
    let mut bus = Bus::new(Cartridge::new(image).unwrap(), region);
    bus.advance_machine(12_345 * MAIN_CPU_DIVISOR + 17).unwrap();
    bus.cpu_clock = CpuClocks(12_345);
    Arc::make_mut(&mut bus.cartridge.bytes)[..8]
        .copy_from_slice(&[0, 0xff, 0x80, 0, 0, 0xff, 2, 0]);
    bus.ram[0x200..0x20c].copy_from_slice(&[
        0x52, 0x79, 0, 0xff, 3, 0, 0x4e, 0x71, 0x4e, 0x71, 0x4e, 0x71,
    ]);
    bus.ram[0x300..0x302].copy_from_slice(&0x1234u16.to_be_bytes());
    let mut registers = Registers::new();
    registers.pc = 0x100;
    registers.ssp = 0xff1000;
    registers.usp = 0xfeedcafe;
    for (index, value) in registers.d.iter_mut().chain(&mut registers.a).enumerate() {
        *value = 0x12340000 + index as u32 * 0x103;
    }
    registers.set_status_register(if supervisor { 0x2115 } else { 0x0115 });
    let cpu = PrefetchCpu::from_prefetched(registers, [0x4e71; 2], bus.cpu_clock).unwrap();
    (cpu, bus)
}

fn target<'a>(
    concurrent: bool,
    bus: &'a mut Bus,
    cpu: &'a mut Cpu,
    schedule: &'a mut SoundSchedule,
) -> Target<'a> {
    if concurrent {
        Target::Scheduled(ScheduledBus::new(bus, cpu, schedule))
    } else {
        Target::Direct(bus)
    }
}

fn access(address: u32) -> BusAccess {
    BusAccess {
        logical_address: address,
        bus_address: address,
        function_code: FunctionCode::SupervisorProgram,
        kind: AccessKind::ResetVector,
        lanes: ByteLanes::Both,
    }
}

fn bytes(cpu: &PrefetchCpu) -> Vec<u8> {
    let mut writer = StateWriter::new();
    cpu.write_boundary_state(&mut writer).unwrap();
    writer.into_bytes()
}

fn record(hash: &mut u64, value: impl std::fmt::Debug) {
    for byte in format!("{value:?}").bytes() {
        *hash = (*hash ^ u64::from(byte)).wrapping_mul(0x100000001b3);
    }
}

fn emit(label: &str, hash: u64, cases: usize) {
    #[cfg(target_arch = "wasm32")]
    wasm_bindgen_test::console_log!("{label} {hash:016x} cases={cases}");
    #[cfg(not(target_arch = "wasm32"))]
    println!("{label} {hash:016x} cases={cases}");
}

fn replay(bus: &mut impl ClockedBus<Error = BusError>, events: &[Event]) {
    for event in events {
        match event {
            Event::Idle(start, clocks) => bus.idle(*start, *clocks),
            Event::Read(access, start, report) => {
                let actual = bus.read_cycle(*access, *start);
                assert_eq!(actual.wait_clocks, 0);
                assert_eq!(
                    report.outcome,
                    EntryOutcome::Complete(actual.result.unwrap())
                );
            }
        }
    }
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn reset_adapter_live_vectors_handler_and_production_controls() {
    let mut hash = 0xcbf29ce484222325;
    let mut cases = 0;
    for region in [Region::NtscUs, Region::NtscJapan, Region::Pal] {
        for concurrent in [false, true] {
            for supervisor in [false, true] {
                let (mut cpu, mut bus) = fixture(region, supervisor);
                let before = cpu.registers;
                let origin = cpu.clock();
                let master = bus.now;
                let mut sound = Cpu::new();
                let mut schedule = SoundSchedule::default();
                let (mut control, mut control_sound, mut control_schedule) =
                    (bus.clone(), sound.clone(), schedule.clone());
                cpu.check_external_reset_boundary().unwrap();
                let mut recorded = Recorded {
                    entry: ResetEntry::new(target(concurrent, &mut bus, &mut sound, &mut schedule))
                        .unwrap(),
                    events: vec![],
                };
                let result = cpu
                    .external_warm_reset(
                        &mut recorded,
                        ExternalResetBoundary {
                            sampled_release: origin,
                            qualification: ResetQualification::QualifiedSafeAssertionAndRelease,
                        },
                    )
                    .unwrap();
                assert_eq!(result.clocks, 40);
                assert_eq!(recorded.entry.failure, None);
                assert_eq!(recorded.entry.idle_failure, None);
                assert_eq!(recorded.events.len(), 8);
                assert_eq!(recorded.events[0], Event::Idle(origin, 14));
                assert_eq!(recorded.events[6], Event::Idle(CpuClocks(origin.0 + 34), 2));
                let reads: Vec<_> = recorded
                    .events
                    .iter()
                    .filter_map(|event| match event {
                        Event::Read(access, start, report) => {
                            Some((*access, *start, report.clone()))
                        }
                        _ => None,
                    })
                    .collect();
                for (index, (access, start, report)) in reads.iter().enumerate() {
                    assert_eq!(access.bus_address, [0, 2, 4, 6, 0xff0200, 0xff0202][index]);
                    assert_eq!(start.0 - origin.0, [14, 18, 22, 26, 30, 36][index]);
                    assert_eq!(access.function_code, FunctionCode::SupervisorProgram);
                    assert_eq!(access.lanes, ByteLanes::Both);
                    assert_eq!(
                        access.kind,
                        if index < 4 {
                            AccessKind::ResetVector
                        } else {
                            AccessKind::Prefetch
                        }
                    );
                    assert_eq!(report.elapsed, 4);
                }
                let events = recorded.events.clone();
                drop(recorded);
                if concurrent {
                    replay(
                        &mut ScheduledBus::new(
                            &mut control,
                            &mut control_sound,
                            &mut control_schedule,
                        ),
                        &events,
                    );
                } else {
                    replay(&mut control, &events);
                }
                assert_eq!(bus, control);
                assert_eq!(sound, control_sound);
                assert_eq!(schedule, control_schedule);
                assert_eq!(bus.now.get() - master.get(), 280);
                assert_eq!(bus.cpu_clock, cpu.clock());
                assert_eq!(bus.vdp.now(), bus.now);
                assert_eq!(bus.cartridge.initial.pc, 0x100);
                assert_eq!(cpu.registers.pc, 0xff0200);
                assert_eq!(cpu.registers.ssp, 0xff8000);
                assert_eq!(cpu.registers.d, before.d);
                assert_eq!(cpu.registers.a, before.a);
                assert_eq!(cpu.registers.usp, before.usp);
                assert_eq!(cpu.registers.status_register(), 0x2715);
                assert_eq!(cpu.queued_words(), Some([0x5279, 0x00ff]));
                assert_eq!(&bus.ram[0x300..0x302], &0x1234u16.to_be_bytes());
                assert_eq!(bus.reset_pulses, 0);
                assert!(!bus.reset_output);
                cpu.finish_external_reset_boundary().unwrap();
                let encoded = bytes(&cpu);
                let mut restored = PrefetchCpu::default();
                restored
                    .read_boundary_state(&mut StateReader::new(&encoded))
                    .unwrap();
                assert_eq!(restored, cpu);
                let mut restored_bus = bus.clone();
                let mut restored_sound = sound.clone();
                let mut restored_schedule = schedule.clone();
                if concurrent {
                    cpu.step_boundary(&mut ScheduledBus::new(&mut bus, &mut sound, &mut schedule))
                        .unwrap();
                    restored
                        .step_boundary(&mut ScheduledBus::new(
                            &mut restored_bus,
                            &mut restored_sound,
                            &mut restored_schedule,
                        ))
                        .unwrap();
                } else {
                    cpu.step_boundary(&mut bus).unwrap();
                    restored.step_boundary(&mut restored_bus).unwrap();
                }
                assert_eq!(&bus.ram[0x300..0x302], &0x1235u16.to_be_bytes());
                assert_eq!(restored, cpu);
                assert_eq!(restored_bus, bus);
                assert_eq!(restored_sound, sound);
                assert_eq!(restored_schedule, schedule);
                record(
                    &mut hash,
                    (
                        region,
                        concurrent,
                        supervisor,
                        events,
                        encoded,
                        bytes(&cpu),
                        bus.now,
                        bus.cpu_clock,
                    ),
                );
                cases += 1;
            }
        }
    }
    emit("md-reset-adapter-row-a-v1", hash, cases);
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn reset_adapter_measured_failures_and_callback_budget() {
    bounded_odd_reset_rejection();
    bounded_switched_reset_rejection();
    let mut hash = 0xcbf29ce484222325;
    let mut cases = 0;
    for concurrent in [false, true] {
        for mismatch in [false, true] {
            let (_, mut bus) = fixture(Region::NtscUs, true);
            let mut sound = Cpu::new();
            let mut schedule = SoundSchedule::default();
            let clock = bus.cpu_clock;
            let master = bus.now;
            let mut entry =
                ResetEntry::new(target(concurrent, &mut bus, &mut sound, &mut schedule)).unwrap();
            let request = CpuClocks(clock.0 + u64::from(mismatch));
            let report = entry.read_entry(access(0x400000), request);
            let expected = if mismatch {
                ResetBusError::Bus(BusError::ClockMismatch {
                    expected: clock,
                    actual: request,
                })
            } else {
                ResetBusError::Bus(BusError::UnsupportedAccess {
                    address: 0x400000,
                    direction: Direction::Read,
                })
            };
            assert_eq!(
                report,
                EntryReport {
                    elapsed: if mismatch { 0 } else { 4 },
                    outcome: EntryOutcome::HostFailure(expected.clone())
                }
            );
            let after = entry.target.bus().clone();
            assert_eq!(
                entry.read_entry(access(0), clock),
                EntryReport {
                    elapsed: 0,
                    outcome: EntryOutcome::HostFailure(expected.clone())
                }
            );
            assert_eq!(entry.target.bus(), &after);
            assert_eq!(
                after.now.get() - master.get(),
                u64::from(report.elapsed) * MAIN_CPU_DIVISOR
            );
            record(&mut hash, (concurrent, mismatch, report));
            cases += 1;
        }
        let (_, mut bus) = fixture(Region::NtscUs, true);
        let mut sound = Cpu::new();
        let mut schedule = SoundSchedule::default();
        let clock = bus.cpu_clock;
        let mut entry =
            ResetEntry::new(target(concurrent, &mut bus, &mut sound, &mut schedule)).unwrap();
        let before = entry.target.bus().clone();
        assert_eq!(
            entry
                .detect_address_error(access(1), Direction::Read, clock)
                .outcome,
            EntryOutcome::TimingUnavailable
        );
        assert_eq!(
            entry.write_entry(access(0xff0300), 0xffff, clock),
            EntryReport {
                elapsed: 0,
                outcome: EntryOutcome::HostFailure(ResetBusError::UnsupportedWrite)
            }
        );
        assert_eq!(entry.target.bus(), &before);
        record(&mut hash, (concurrent, entry.failure));
        cases += 1;
    }
    for (completed, requested, accepted) in [
        (0, u32::MAX, true),
        (u64::from(u32::MAX) - 4, 4, true),
        (u64::from(u32::MAX) - 3, 4, false),
        (u64::from(u32::MAX), 0, true),
        (u64::from(u32::MAX), 1, false),
        (u64::MAX - 1, 4, false),
    ] {
        let result = budget(CpuClocks(0), CpuClocks(completed), requested);
        assert_eq!(result.is_ok(), accepted);
        record(&mut hash, (completed, requested, result));
        cases += 1;
    }
    assert_eq!(
        budget(CpuClocks(2), CpuClocks(1), 0),
        Err(ResetBusError::ClockGeometry)
    );
    cases += 1;
    emit("md-reset-adapter-callback-v1", hash, cases);
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn reset_adapter_actual_idle_failure_freezes_without_next_service() {
    let mut hash = 0xcbf29ce484222325;
    let mut cases = 0;
    for concurrent in [false, true] {
        let (mut cpu, mut bus) = fixture(Region::NtscUs, true);
        bus.now = MasterTicks::new(u64::MAX - 1);
        let mut sound = Cpu::new();
        let mut schedule = SoundSchedule::default();
        let origin = cpu.clock();
        let before = cpu.registers;
        let mut entry =
            ResetEntry::new(target(concurrent, &mut bus, &mut sound, &mut schedule)).unwrap();
        let boundary = ExternalResetBoundary {
            sampled_release: origin,
            qualification: ResetQualification::QualifiedSafeAssertionAndRelease,
        };
        let failure = cpu.external_warm_reset(&mut entry, boundary).unwrap_err();
        let cause = ResetBusError::Bus(BusError::ClockOverflow);
        assert!(matches!(&failure, EntryFault::HostFailure { source, .. } if *source == cause));
        assert_eq!(
            entry.idle_failure,
            Some(IdleFailure {
                start: origin,
                requested: 14,
                completed: 0,
                source: cause
            })
        );
        assert_eq!(cpu.registers, before);
        assert_eq!(cpu.clock().0, origin.0 + 14);
        assert_eq!(entry.target.bus().cpu_clock, origin);
        assert_eq!(entry.target.bus().now.get(), u64::MAX - 1);
        assert!(entry.target.bus().clock_fault);
        assert_eq!(cpu.queued_words(), None);
        assert!(cpu.finish_external_reset_boundary().is_err());
        let terminal = entry.target.bus().clone();
        assert_eq!(
            cpu.external_warm_reset(&mut entry, boundary),
            Err(EntryFault::HostFrozen)
        );
        assert_eq!(entry.target.bus(), &terminal);
        assert!(bytes_rejected(&cpu));
        record(
            &mut hash,
            (
                concurrent,
                failure,
                entry.idle_failure,
                cpu.clock(),
                terminal.cpu_clock,
                terminal.now,
            ),
        );
        cases += 1;
    }
    emit("md-reset-adapter-idle-v1", hash, cases);
}

fn bounded_odd_reset_rejection() {
    for concurrent in [false, true] {
        let (_, mut bus) = fixture(Region::NtscUs, true);
        bus.cartridge.save = Some(crate::cartridge::SaveMemory::bounded_odd_for_test());
        let mut cpu = Cpu::new();
        let mut schedule = SoundSchedule::default();
        let before = bus.clone();
        let before_cpu = cpu.clone();
        let before_schedule = schedule.clone();
        let clock = bus.cpu_clock;
        let access = BusAccess {
            logical_address: 0x200001,
            bus_address: 0x200000,
            kind: AccessKind::Data,
            lanes: ByteLanes::Lower,
            function_code: FunctionCode::SupervisorData,
        };
        assert!(
            target(concurrent, &mut bus, &mut cpu, &mut schedule)
                .read(access, clock)
                .is_err()
        );
        assert_eq!(bus, before);
        assert_eq!(cpu, before_cpu);
        assert_eq!(schedule, before_schedule);
    }
}

fn bounded_switched_reset_rejection() {
    for concurrent in [false, true] {
        for (profile, mode) in [(0, 0), (0, 1), (0, 3), (1, 0), (2, 0), (2, 1), (2, 3)] {
            let (_, mut bus) = fixture(Region::NtscUs, true);
            let mut save = if profile == 2 {
                crate::cartridge::SaveMemory::bounded_bytes_for_test()
            } else if profile == 0 {
                crate::cartridge::SaveMemory::bounded_switched_for_test()
            } else {
                crate::cartridge::SaveMemory::reserved_switched_for_test()
            };
            if profile != 1 {
                assert!(save.control(mode, false));
            }
            bus.cartridge.save = Some(save);
            let mut cpu = Cpu::new();
            let mut schedule = SoundSchedule::default();
            let before = bus.clone();
            let before_cpu = cpu.clone();
            let before_schedule = schedule.clone();
            let clock = bus.cpu_clock;
            let access = BusAccess {
                logical_address: 0x200005,
                bus_address: 0x200004,
                kind: AccessKind::Data,
                lanes: ByteLanes::Lower,
                function_code: FunctionCode::SupervisorData,
            };
            assert!(
                target(concurrent, &mut bus, &mut cpu, &mut schedule)
                    .read(access, clock)
                    .is_err()
            );
            assert_eq!(bus, before);
            assert_eq!(cpu, before_cpu);
            assert_eq!(schedule, before_schedule);
        }
    }
}

fn bytes_rejected(cpu: &PrefetchCpu) -> bool {
    cpu.write_boundary_state(&mut StateWriter::new()).is_err()
}
