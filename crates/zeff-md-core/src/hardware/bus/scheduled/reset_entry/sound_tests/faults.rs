use super::*;
use std::sync::Arc;
use zeff_emu_common::save_state::StateWriter;
use zeff_m68k::timing::physical::{
    CriticalFault, PhysicalEntryState, PhysicalInspection, TimingQualification,
};
use zeff_m68k::timing::reset_boundary::ExternalResetBoundaryError;

struct Checked<'a> {
    entry: ResetEntry<'a>,
    reads: Vec<(BusAccess, CpuClocks, EntryReport<u16, ResetBusError>)>,
    detects: Vec<(
        BusAccess,
        Direction,
        CpuClocks,
        EntryReport<(), ResetBusError>,
    )>,
}

impl EntryBus for Checked<'_> {
    type Error = ResetBusError;
    fn read_entry(&mut self, access: BusAccess, start: CpuClocks) -> EntryReport<u16, Self::Error> {
        let report = self.entry.read_entry(access, start);
        self.reads.push((access, start, report.clone()));
        report
    }
    fn idle_entry(&mut self, start: CpuClocks, clocks: u32) {
        self.entry.idle_entry(start, clocks);
    }
    fn write_entry(
        &mut self,
        access: BusAccess,
        value: u16,
        start: CpuClocks,
    ) -> EntryReport<(), Self::Error> {
        self.entry.write_entry(access, value, start)
    }
    fn detect_address_error(
        &mut self,
        access: BusAccess,
        direction: Direction,
        start: CpuClocks,
    ) -> EntryReport<(), Self::Error> {
        let before = self.entry.target.bus().clone();
        let report = self.entry.detect_address_error(access, direction, start);
        assert_eq!(&before, self.entry.target.bus());
        self.detects
            .push((access, direction, start, report.clone()));
        report
    }
}

fn checked(rig: &mut Rig, concurrent: bool) -> Checked<'_> {
    Checked {
        entry: ResetEntry::new(rig.target(concurrent)).unwrap(),
        reads: vec![],
        detects: vec![],
    }
}

fn repeat_reset(cpu: &mut PrefetchCpu, bus: &mut Checked<'_>) {
    let before = bus.entry.target.bus().clone();
    let sound = match &bus.entry.target {
        Target::Scheduled(target) => Some((target.cpu.clone(), target.schedule.clone())),
        Target::Direct(_) => None,
    };
    let counts = (bus.reads.len(), bus.detects.len());
    assert_eq!(reset(cpu, bus), Err(EntryFault::HostFrozen));
    assert_eq!(counts, (bus.reads.len(), bus.detects.len()));
    assert_eq!(&before, bus.entry.target.bus());
    if let (Some((cpu, schedule)), Target::Scheduled(target)) = (sound, &bus.entry.target) {
        assert_eq!((&cpu, &schedule), (&*target.cpu, &*target.schedule));
    }
}

fn reject_frozen(cpu: &mut PrefetchCpu, rig: &mut Rig, concurrent: bool) {
    assert_eq!(cpu.queued_words(), None);
    assert_eq!(
        cpu.check_external_reset_boundary(),
        Err(ExternalResetBoundaryError::HostFrozen)
    );
    assert_eq!(
        cpu.finish_external_reset_boundary(),
        Err(ExternalResetBoundaryError::HostFrozen)
    );
    assert!(cpu.write_boundary_state(&mut StateWriter::new()).is_err());
    let before = rig.clone();
    if concurrent {
        assert!(matches!(
            cpu.step_boundary(&mut rig.scheduled()),
            Err(TimingFault::Frozen)
        ));
    } else {
        assert!(matches!(
            cpu.step_boundary(&mut rig.bus),
            Err(TimingFault::Frozen)
        ));
    }
    assert_eq!(&before, rig);
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn reset_six_actual_sound_read_faults_and_mapped_handler_failures() {
    let mut hash = 0xcbf29ce484222325;
    let mut cases = 0;
    for (index, cut) in [278, 250, 222, 194, 166, 124].into_iter().enumerate() {
        for batched in [false, true] {
            let mut rig = Rig::new(&[0, 0, 0, 0, 0x3a, 0, 0x7f], cut, false, batched, 0, false);
            let mut cpu = rig.main();
            let before = cpu.registers;
            let origin = cpu.clock();
            let mut bus = checked(&mut rig, true);
            let failure = reset(&mut cpu, &mut bus).unwrap_err();
            assert_eq!(bus.reads.len(), index + 1);
            assert!(bus.detects.is_empty());
            let (access, _, report) = bus.reads.last().unwrap();
            assert_eq!(access.bus_address, [0, 2, 4, 6, 0x100, 0x102][index]);
            assert_eq!(access.function_code, FunctionCode::SupervisorProgram);
            assert_eq!(
                access.kind,
                if index < 4 {
                    AccessKind::ResetVector
                } else {
                    AccessKind::Prefetch
                }
            );
            assert_eq!(report.elapsed, 4);
            let source = ResetBusError::Sound(SoundFault::UnsupportedAccess {
                address: 0x7f00,
                direction: Direction::Read,
            });
            assert_eq!(report.outcome, EntryOutcome::HostFailure(source.clone()));
            assert!(
                matches!(&failure, EntryFault::HostFailure { access: actual, source: cause, .. } if actual==access && *cause==source)
            );
            assert_eq!(bus.entry.failure, Some(source));
            assert_eq!(bus.entry.idle_failure, None);
            assert_eq!(cpu.registers, before);
            assert_eq!(cpu.clock().0 - origin.0, [18, 22, 26, 30, 34, 40][index]);
            assert_eq!(bus.entry.target.bus().cpu_clock, cpu.clock());
            let reads = bus.reads.clone();
            repeat_reset(&mut cpu, &mut bus);
            drop(bus);
            assert_eq!(
                rig.bus.now.get() - cut,
                (cpu.clock().0 - origin.0) * MAIN_CPU_DIVISOR
            );
            assert!(rig.schedule.fault.is_some());
            reject_frozen(&mut cpu, &mut rig, true);
            rig.finish_batch(batched);
            record(
                &mut hash,
                (
                    index,
                    batched,
                    failure,
                    reads,
                    cpu.clock(),
                    rig.bus.now,
                    rig.sound.cycles(),
                ),
            );
            cases += 1;
        }
    }
    for (target, reads_expected) in [(0x400000u32, 5), (0x3ffffe, 6)] {
        for concurrent in [false, true] {
            let mut rig = Rig::new(&[0x76], 1, false, concurrent, 0, false);
            Arc::make_mut(&mut rig.bus.cartridge.bytes)[4..8]
                .copy_from_slice(&target.to_be_bytes());
            let mut cpu = rig.main();
            let before = cpu.registers;
            let mut bus = checked(&mut rig, concurrent);
            let failure = reset(&mut cpu, &mut bus).unwrap_err();
            assert_eq!(bus.reads.len(), reads_expected);
            let (access, _, report) = bus.reads.last().unwrap();
            assert_eq!(access.bus_address, 0x400000);
            assert_eq!(access.kind, AccessKind::Prefetch);
            assert_eq!(
                report,
                &EntryReport {
                    elapsed: 4,
                    outcome: EntryOutcome::HostFailure(ResetBusError::Bus(
                        BusError::UnsupportedAccess {
                            address: 0x400000,
                            direction: Direction::Read
                        }
                    ))
                }
            );
            assert!(matches!(
                &failure,
                EntryFault::HostFailure {
                    source: ResetBusError::Bus(BusError::UnsupportedAccess {
                        address: 0x400000,
                        ..
                    }),
                    ..
                }
            ));
            assert_eq!(cpu.registers, before);
            assert_eq!(bus.entry.idle_failure, None);
            assert_eq!(bus.entry.target.bus().cpu_clock, cpu.clock());
            let reads = bus.reads.clone();
            repeat_reset(&mut cpu, &mut bus);
            drop(bus);
            assert_eq!(rig.schedule.fault, None);
            reject_frozen(&mut cpu, &mut rig, concurrent);
            rig.finish_batch(concurrent);
            record(
                &mut hash,
                (target, concurrent, failure, reads, cpu.clock(), rig.bus.now),
            );
            cases += 1;
        }
    }
    emit("md-reset-read-faults-v1", hash, cases);
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn reset_odd_live_handler_uses_adapter_detector_without_transfer() {
    let mut hash = 0xcbf29ce484222325;
    let mut cases = 0;
    for region in [Region::NtscUs, Region::NtscJapan, Region::Pal] {
        for concurrent in [false, true] {
            let mut rig = Rig::new(&[0x76], 1, false, concurrent, 0, false);
            rig.bus.region = region;
            rig.bus.audio = crate::hardware::audio::Audio::new(region);
            rig.bus.audio.advance_to(rig.bus.now.get());
            if concurrent {
                rig.bus.audio.begin_batch();
            }
            Arc::make_mut(&mut rig.bus.cartridge.bytes)[4..8]
                .copy_from_slice(&0x101u32.to_be_bytes());
            let mut cpu = rig.main();
            let before = cpu.registers;
            let origin = cpu.clock();
            let mut bus = checked(&mut rig, concurrent);
            let failure = reset(&mut cpu, &mut bus).unwrap_err();
            assert_eq!(bus.reads.len(), 4);
            assert_eq!(bus.detects.len(), 1);
            let (access, direction, at, report) = bus.detects[0].clone();
            assert_eq!(
                (
                    access.logical_address,
                    access.bus_address,
                    access.kind,
                    access.function_code,
                    direction
                ),
                (
                    0x101,
                    0x100,
                    AccessKind::Prefetch,
                    FunctionCode::SupervisorProgram,
                    Direction::Read
                )
            );
            assert_eq!(at.0 - origin.0, 30);
            assert_eq!(
                report,
                EntryReport {
                    elapsed: 0,
                    outcome: EntryOutcome::TimingUnavailable
                }
            );
            let cause = CriticalFault::AddressError { access, direction };
            assert_eq!(failure, EntryFault::TimingUnavailable(cause));
            assert_eq!(
                cpu.physical_entry_state(),
                PhysicalEntryState::DoubleFaultHalted {
                    cause,
                    timing: TimingQualification::Unavailable
                }
            );
            assert_eq!(cpu.physical_inspection(), PhysicalInspection::Unavailable);
            assert_eq!(cpu.queued_words(), None);
            assert_eq!(cpu.registers, before);
            assert_eq!(bus.entry.failure, None);
            assert_eq!(bus.entry.target.bus().cpu_clock, cpu.clock());
            drop(bus);
            assert_eq!(
                cpu.check_external_reset_boundary(),
                Err(ExternalResetBoundaryError::NotOrdinaryBoundary)
            );
            assert!(cpu.finish_external_reset_boundary().is_err());
            assert!(cpu.write_boundary_state(&mut StateWriter::new()).is_err());
            let terminal = rig.clone();
            if concurrent {
                assert!(matches!(
                    cpu.step_boundary(&mut rig.scheduled()),
                    Err(TimingFault::DoubleFaultHalted)
                ));
            } else {
                assert!(matches!(
                    cpu.step_boundary(&mut rig.bus),
                    Err(TimingFault::DoubleFaultHalted)
                ));
            }
            assert_eq!(rig, terminal);
            rig.finish_batch(concurrent);
            record(
                &mut hash,
                (region, concurrent, failure, cpu.clock(), rig.bus.now),
            );
            cases += 1;
        }
    }
    emit("md-reset-odd-handler-v1", hash, cases);
}
