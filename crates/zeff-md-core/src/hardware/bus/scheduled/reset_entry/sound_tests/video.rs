use super::*;

fn fixture(region: Region, h40: bool, dma: bool) -> Rig {
    let mut rig = Rig::new(&[0x76], 1, false, false, 0, false);
    rig.bus.region = region;
    rig.bus.audio = crate::hardware::audio::Audio::new(region);
    rig.bus.audio.advance_to(rig.bus.now.get());
    for (index, value) in [
        (1, 0x54),
        (12, if h40 { 0x81 } else { 0 }),
        (15, 2),
        (19, 17),
        (20, 0),
        (21, 0),
        (22, 0xc0),
        (23, 0x7f),
    ] {
        rig.advance(true, 1);
        let write = rig.bus.vdp.queue_register(index, value, region).unwrap();
        rig.advance(true, write.publish_at.get() - rig.bus.now.get());
    }
    let edge = rig.bus.now.get().div_ceil(VDP_LINE_TICKS) * VDP_LINE_TICKS;
    rig.advance(true, edge - 1 - rig.bus.now.get());
    for (index, byte) in rig.bus.ram[0x8000..0x8022].iter_mut().enumerate() {
        *byte = index as u8 ^ 0x5a;
    }
    rig.bus.vdp.write_control(0x4400, region).unwrap();
    rig.bus
        .vdp
        .write_control(if dma { 0x80 } else { 0 }, region)
        .unwrap();
    if dma {
        assert!(rig.bus.external_dma_wait().unwrap().is_some());
    } else {
        for word in [0x1357, 0x2468, 0x9abc, 0xdef0] {
            rig.bus.vdp.write_data(word, region).unwrap();
        }
        assert_eq!(rig.bus.vdp.fifo().len(), 4);
    }
    rig.bus
        .video
        .queue(MasterTicks::new(edge), rig.bus.now)
        .unwrap();
    assert_eq!(rig.bus.vdp.now(), rig.bus.now);
    assert!(rig.schedule.fault.is_none());
    rig
}

fn assert_destination(rig: &Rig, dma: bool) {
    let expected: Vec<u8> = if dma {
        rig.bus.ram[0x8000..0x8022].to_vec()
    } else {
        vec![0x13, 0x57, 0x24, 0x68, 0x9a, 0xbc, 0xde, 0xf0]
    };
    assert_eq!(
        &rig.bus.vdp.memory().vram()[0x400..0x400 + expected.len()],
        expected
    );
    assert!(rig.bus.vdp.fifo().is_empty());
    assert_eq!(rig.bus.external_dma_wait().unwrap(), None);
}

fn mapped_access() -> BusAccess {
    BusAccess {
        logical_address: 0x400000,
        bus_address: 0x400000,
        function_code: FunctionCode::SupervisorProgram,
        kind: AccessKind::Prefetch,
        lanes: ByteLanes::Both,
    }
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn reset_live_video_dma_fifo_and_capture_match_ordinary_callbacks() {
    let mut hash = 0xcbf29ce484222325;
    let mut cases = 0;
    for region in [Region::NtscUs, Region::NtscJapan, Region::Pal] {
        for h40 in [false, true] {
            for dma in [false, true] {
                for concurrent in [false, true] {
                    let mut actual = fixture(region, h40, dma);
                    let mut control = actual.clone();
                    let retained = (
                        actual.bus.cartridge.clone(),
                        actual.bus.ram.clone(),
                        actual.bus.input.clone(),
                        actual.bus.reset_pulses,
                    );
                    let mut cpu = actual.main();
                    let origin = (cpu.clock(), actual.bus.now);
                    let mut bus = Recorded {
                        entry: ResetEntry::new(actual.target(concurrent)).unwrap(),
                        events: vec![],
                    };
                    let step = reset(&mut cpu, &mut bus).unwrap();
                    assert_eq!(bus.entry.failure, None);
                    let events = bus.events.clone();
                    assert_eq!(events.len(), 8);
                    drop(bus);
                    control.replay(concurrent, &events);
                    assert_eq!(actual, control);
                    assert_eq!(cpu.clock(), actual.bus.cpu_clock);
                    assert_eq!(cpu.clock().0 - origin.0.0, step.clocks);
                    assert_eq!(
                        actual.bus.now.get() - origin.1.get(),
                        step.clocks * MAIN_CPU_DIVISOR
                    );
                    assert_eq!(actual.bus.vdp.now(), actual.bus.now);
                    assert_eq!(
                        (
                            &actual.bus.cartridge,
                            &actual.bus.ram,
                            &actual.bus.input,
                            actual.bus.reset_pulses
                        ),
                        (&retained.0, &retained.1, &retained.2, retained.3)
                    );
                    cpu.finish_external_reset_boundary().unwrap();
                    assert!(actual.bus.video.take().is_some());
                    assert!(control.bus.video.take().is_some());
                    actual.advance(concurrent, 50_000);
                    control.advance(concurrent, 50_000);
                    assert_eq!(actual, control);
                    assert_destination(&actual, dma);
                    assert_eq!(actual.pcm(), control.pcm());
                    record(
                        &mut hash,
                        (region, h40, dma, concurrent, step, events, actual.bus.now),
                    );
                    cases += 1;
                }
            }
        }
    }
    emit("md-reset-video-overlap-v1", hash, cases);
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn reset_callback_mapping_failure_keeps_completed_dma_wait() {
    let mut hash = 0xcbf29ce484222325;
    let mut cases = 0;
    for region in [Region::NtscUs, Region::NtscJapan, Region::Pal] {
        for h40 in [false, true] {
            for concurrent in [false, true] {
                let mut actual = fixture(region, h40, true);
                let mut control = actual.clone();
                let clock = actual.bus.cpu_clock;
                let expected_wait = actual.bus.external_dma_wait().unwrap().unwrap().0;
                assert!(expected_wait > 0);
                let source = BusError::UnsupportedAccess {
                    address: 0x400000,
                    direction: Direction::Read,
                };
                let report = {
                    let mut entry = ResetEntry::new(actual.target(concurrent)).unwrap();
                    let report = entry.read_entry(mapped_access(), clock);
                    assert_eq!(
                        report.outcome,
                        EntryOutcome::HostFailure(ResetBusError::Bus(source.clone()))
                    );
                    assert_eq!(report.elapsed, expected_wait + 4);
                    assert_eq!(entry.idle_failure, None);
                    let before = entry.target.bus().clone();
                    assert_eq!(entry.read_entry(mapped_access(), clock).elapsed, 0);
                    assert_eq!(entry.target.bus(), &before);
                    report
                };
                let ordinary = if concurrent {
                    control.scheduled().read_cycle(mapped_access(), clock)
                } else {
                    control.bus.read_cycle(mapped_access(), clock)
                };
                assert_eq!(ordinary.result, Err(source));
                assert_eq!(ordinary.wait_clocks, expected_wait);
                assert_eq!(actual, control);
                assert_eq!(actual.bus.external_dma_wait().unwrap(), None);
                assert!(actual.bus.video.take().is_some());
                assert!(control.bus.video.take().is_some());
                actual.advance(concurrent, 50_000);
                control.advance(concurrent, 50_000);
                assert_destination(&actual, true);
                assert_eq!(actual, control);
                assert_eq!(actual.pcm(), control.pcm());
                record(&mut hash, (region, h40, concurrent, report, actual.bus.now));
                cases += 1;
            }
        }
    }
    emit("md-reset-dma-failed-callback-v1", hash, cases);
}

#[cfg(feature = "sound-shadow-observer")]
mod observer {
    use super::*;
    use crate::hardware::sound::observer::{Metric, Session, Verification};

    fn execute(case: u8, ticks: u64) -> (String, Vec<u32>) {
        let mut rig = fixture(Region::NtscUs, true, case != 0);
        let mut cpu = rig.main();
        if case == 2 {
            let clock = cpu.clock();
            let mut entry = ResetEntry::new(rig.target(true)).unwrap();
            assert!(matches!(
                entry.read_entry(mapped_access(), clock).outcome,
                EntryOutcome::HostFailure(ResetBusError::Bus(BusError::UnsupportedAccess { .. }))
            ));
        } else {
            {
                let mut entry = ResetEntry::new(rig.target(true)).unwrap();
                reset(&mut cpu, &mut entry).unwrap();
            }
            cpu.finish_external_reset_boundary().unwrap();
        }
        rig.close_epoch();
        rig.advance(true, ticks);
        if ticks == 50_000 {
            assert_destination(&rig, case != 0);
        }
        let _verification = Verification::begin();
        (format!("{cpu:?}|{rig:?}"), rig.pcm())
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    fn observer_video_reset_short_and_long_reused_inventory() {
        let mut hash = 0xcbf29ce484222325;
        for case in 0..3 {
            for ticks in [1_000, 50_000] {
                let expected = execute(case, ticks);
                let session = Session::begin();
                assert_eq!(execute(case, ticks), expected);
                let report = session.finish();
                assert!(!report.full_coverage);
                assert_eq!(report.scope, "DirectedOwnerThreadOnly");
                assert_eq!((report.live_owners, report.live_images), (0, 0));
                assert_eq!(
                    report.count(Metric::OwnersCreated),
                    report.count(Metric::OwnersClosed)
                );
                assert_eq!(
                    report.count(Metric::ImagesCreated),
                    report.count(Metric::ImagesClosed)
                );
                assert!(report.count(Metric::Attempts) > 0);
                assert_eq!(
                    report.count(Metric::Attempts),
                    report.count(Metric::Successes) + report.count(Metric::Failures)
                );
                assert_eq!(report.count(Metric::Failures), 0);
                assert!(report.closed(), "{report:?}");
                if ticks == 50_000 {
                    assert!(report.count(Metric::OwnersCreated) > 1_024);
                }
                record(&mut hash, (case, ticks, report));
            }
        }
        emit("md-observer-video-reset-reuse-v1", hash, 6);
    }
}
