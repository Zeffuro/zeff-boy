use super::*;
use crate::hardware::sound::{BankAccess, schedule::PendingSound};
use zeff_m68k::bus::FunctionCode;
use zeff_m68k::timing::physical::{EntryFault, ExternalResetBoundary, ResetQualification};
use zeff_m68k::timing::{PrefetchCpu, TimingFault};

mod admission;
mod faults;
#[cfg(feature = "sound-shadow-observer")]
mod lifecycle;
mod rig;
mod scope;
mod video;
use rig::Rig;

#[derive(Clone, Debug)]
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
    fn idle_entry(&mut self, start: CpuClocks, clocks: u32) {
        self.events.push(Event::Idle(start, clocks));
        self.entry.idle_entry(start, clocks);
    }
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
}

fn reset(
    cpu: &mut PrefetchCpu,
    bus: &mut impl EntryBus<Error = ResetBusError>,
) -> Result<zeff_m68k::timing::physical::EntryStep, EntryFault<ResetBusError>> {
    cpu.external_warm_reset(
        bus,
        ExternalResetBoundary {
            sampled_release: cpu.clock(),
            qualification: ResetQualification::QualifiedSafeAssertionAndRelease,
        },
    )
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

fn pending(kind: u8, cut: u64, batched: bool) -> Rig {
    let (program, bank) = match kind {
        0 => ([0x32, 0x00, 0x01, 0x76], 0),
        1 => ([0x32, 0x00, 0x80, 0x76], 0x1ff),
        2 => ([0x32, 0x00, 0x80, 0x76], 0x80),
        _ => unreachable!(),
    };
    let rig = Rig::new(&program, cut, false, batched, bank, kind == 2);
    assert_eq!(rig.value(kind), if cut < 195 { 0xde } else { 0 });
    if cut < 195 {
        let pending: &PendingSound = rig.schedule.pending.as_ref().unwrap();
        assert_eq!((pending.start, pending.deadline), (150, 195));
        if kind != 0 {
            assert_eq!(pending.reservation().unwrap().ticks, 150..195);
            assert_eq!(
                pending.bank_access,
                Some(BankAccess {
                    address: if kind == 1 { 0xff8000 } else { 0x400000 },
                    direction: Direction::Write,
                    value: 0
                })
            );
        }
    }
    rig
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn reset_active_sound_pending_writes_and_reservation_fences() {
    let mut hash = 0xcbf29ce484222325;
    let mut cases = 0;
    for kind in 0..3 {
        for cut in [194, 195, 196] {
            for concurrent in [false, true] {
                let mut actual = pending(kind, cut, concurrent);
                let mut control = actual.clone();
                let retained = (
                    actual.sound.clone(),
                    actual.schedule.clone(),
                    actual.bus.sound.clone(),
                );
                let mut cpu = actual.main();
                let origin = cpu.clock();
                let mut bus = Recorded {
                    entry: ResetEntry::new(actual.target(concurrent)).unwrap(),
                    events: vec![],
                };
                assert_eq!(reset(&mut cpu, &mut bus).unwrap().clocks, 40);
                assert_eq!(bus.entry.failure, None);
                assert_eq!(bus.events.len(), 8);
                let events = bus.events.clone();
                drop(bus);
                control.replay(concurrent, &events);
                actual.finish_batch(concurrent);
                control.finish_batch(concurrent);
                assert_eq!(actual, control);
                assert_eq!(actual.bus.cpu_clock.0 - origin.0, 40);
                assert_eq!(
                    actual.value(kind),
                    if concurrent || cut >= 195 { 0 } else { 0xde }
                );
                if !concurrent {
                    assert_eq!(
                        (&actual.sound, &actual.schedule, &actual.bus.sound),
                        (&retained.0, &retained.1, &retained.2)
                    );
                }
                cpu.finish_external_reset_boundary().unwrap();
                if kind != 0 {
                    actual.replace_value(kind);
                    control.replace_value(kind);
                }
                actual.advance(concurrent, 50_000);
                control.advance(concurrent, 50_000);
                assert_eq!(
                    actual.value(kind),
                    if kind != 0 {
                        0xa6
                    } else if concurrent || cut >= 195 {
                        0
                    } else {
                        0xde
                    }
                );
                assert_eq!(actual, control);
                let pcm = actual.pcm();
                assert_eq!(pcm, control.pcm());
                assert!(pcm.iter().any(|&v| v & 0x7fffffff != 0));
                record(
                    &mut hash,
                    (
                        kind,
                        cut,
                        concurrent,
                        cpu.registers,
                        cpu.clock(),
                        events,
                        actual.sound.cycles(),
                        pcm,
                    ),
                );
                cases += 1;
            }
        }
    }
    for kind in [1, 2] {
        for cut in [194, 195, 196] {
            let mut actual = pending(kind, cut, false);
            let mut control = actual.clone();
            let clock = actual.bus.cpu_clock;
            let access = BusAccess {
                logical_address: 0,
                bus_address: 0,
                kind: AccessKind::ResetVector,
                lanes: ByteLanes::Both,
                function_code: FunctionCode::SupervisorProgram,
            };
            let report = {
                let mut entry = ResetEntry::new(actual.target(true)).unwrap();
                entry.read_entry(access, clock)
            };
            let ordinary = control.scheduled().read_cycle(access, clock);
            assert_eq!(ordinary.wait_clocks, u32::from(cut == 194));
            assert_eq!(report.elapsed, 4 + ordinary.wait_clocks);
            assert_eq!(
                report.outcome,
                EntryOutcome::Complete(ordinary.result.unwrap())
            );
            assert_eq!(actual.value(kind), 0);
            assert_eq!(actual, control);
            record(
                &mut hash,
                (kind, cut, report, actual.bus.cpu_clock, actual.bus.now),
            );
            cases += 1;
        }
    }
    emit("md-reset-active-pending-v1", hash, cases);
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn reset_active_sound_prepared_and_committed_epochs() {
    let mut hash = 0xcbf29ce484222325;
    let mut cases = 0;
    for cut in [1, 61, 121] {
        for close_before in [false, true] {
            for batched in [false, true] {
                let mut actual = Rig::new(&[0; 32], cut, true, batched, 0, false);
                assert!(actual.schedule.epoch.is_some());
                assert!(actual.schedule.pending.is_none());
                assert_eq!(actual.sound.cycles(), 0);
                let mut control = actual.clone();
                control.close_epoch();
                assert_eq!(control.sound.cycles(), cut / 60 * 4);
                assert!(control.schedule.pending.is_some());
                if close_before {
                    actual.close_epoch();
                }
                let mut cpu = actual.main();
                let mut bus = Recorded {
                    entry: ResetEntry::new(actual.target(true)).unwrap(),
                    events: vec![],
                };
                reset(&mut cpu, &mut bus).unwrap();
                assert_eq!(bus.entry.failure, None);
                let events = bus.events.clone();
                drop(bus);
                control.replay(true, &events);
                actual.close_epoch();
                control.close_epoch();
                actual.finish_batch(batched);
                control.finish_batch(batched);
                assert_eq!(actual, control);
                cpu.finish_external_reset_boundary().unwrap();
                actual.advance(true, 50_000);
                control.advance(true, 50_000);
                assert_eq!(actual, control);
                let pcm = actual.pcm();
                assert_eq!(pcm, control.pcm());
                assert!(pcm.iter().any(|&v| v & 0x7fffffff != 0));
                record(
                    &mut hash,
                    (
                        cut,
                        close_before,
                        batched,
                        cpu.clock(),
                        actual.sound.cycles(),
                        pcm,
                    ),
                );
                cases += 1;
            }
        }
    }
    emit("md-reset-active-epoch-v1", hash, cases);
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn reset_active_sound_faults_at_both_idles_and_read() {
    let mut hash = 0xcbf29ce484222325;
    let mut cases = 0;
    for (nops, cut, idle, requested, completed, read_count) in [
        (0, 60, true, 14, 14, 1),
        (0, 40, false, 0, 0, 1),
        (4, 144, true, 2, 2, 6),
    ] {
        for batched in [false, true] {
            let mut program = vec![0; nops];
            program.extend([0x3a, 0x00, 0x7f]);
            let mut rig = Rig::new(&program, cut, false, batched, 0, false);
            let mut cpu = rig.main();
            let before = cpu.registers;
            let mut bus = Recorded {
                entry: ResetEntry::new(rig.target(true)).unwrap(),
                events: vec![],
            };
            let failure = reset(&mut cpu, &mut bus).unwrap_err();
            let source = ResetBusError::Sound(SoundFault::UnsupportedAccess {
                address: 0x7f00,
                direction: Direction::Read,
            });
            assert!(
                matches!(&failure, EntryFault::HostFailure { source: actual, .. } if *actual == source)
            );
            assert_eq!(bus.entry.failure, Some(source.clone()));
            assert_eq!(
                bus.events
                    .iter()
                    .filter(|e| matches!(e, Event::Read(..)))
                    .count(),
                read_count
            );
            if idle {
                let latched = bus.entry.idle_failure.as_ref().unwrap();
                assert_eq!(
                    (latched.requested, latched.completed),
                    (requested, completed)
                );
                assert_eq!(latched.source, source);
                assert!(bus.entry.target.bus().clock_fault);
                assert!(matches!(
                    bus.events.last(),
                    Some(Event::Read(_, _, EntryReport { elapsed: 0, .. }))
                ));
            } else {
                assert_eq!(bus.entry.idle_failure, None);
                assert!(matches!(
                    bus.events.last(),
                    Some(Event::Read(_, _, EntryReport { elapsed: 4, .. }))
                ));
            }
            assert_eq!(bus.entry.target.bus().cpu_clock, cpu.clock());
            assert_eq!(cpu.registers, before);
            assert_eq!(cpu.queued_words(), None);
            assert!(cpu.finish_external_reset_boundary().is_err());
            let terminal = bus.entry.target.bus().clone();
            assert_eq!(reset(&mut cpu, &mut bus), Err(EntryFault::HostFrozen));
            assert_eq!(bus.entry.target.bus(), &terminal);
            let events = bus.events.clone();
            drop(bus);
            assert!(matches!(
                cpu.step_boundary(&mut rig.scheduled()),
                Err(TimingFault::Frozen)
            ));
            assert_eq!(rig.bus, terminal);
            rig.close_epoch();
            rig.finish_batch(batched);
            assert!(rig.schedule.fault.is_some());
            record(
                &mut hash,
                (
                    nops,
                    cut,
                    batched,
                    failure,
                    events,
                    rig.sound.cycles(),
                    rig.bus.now,
                    rig.bus.cpu_clock,
                ),
            );
            cases += 1;
        }
    }
    emit("md-reset-active-fault-v1", hash, cases);
}
