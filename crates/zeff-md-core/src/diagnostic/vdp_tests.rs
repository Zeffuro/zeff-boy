use super::*;
use crate::hardware::vdp::{VdpDmaSource, VdpPortError, VdpRegisterError, VdpScheduleError};
use crate::{LoadConfig, Region};
use context::TraceContext;
use zeff_emu_common::time::MachineTiming;
use zeff_m68k::bus::{AccessKind, ByteLanes, FunctionCode};
use zeff_m68k::timing::TimingFault;

fn machine(region: Region, concurrent_sound: bool, word: u16) -> Emulator {
    let mut rom = vec![0; 0x1000];
    rom[..8].copy_from_slice(&[0, 0xff, 0xff, 0xfc, 0, 0, 2, 0]);
    let [high, low] = word.to_be_bytes();
    rom[0x200..0x20c]
        .copy_from_slice(&[0x33, 0xfc, high, low, 0, 0xc0, 0, 4, 0x60, 0xfe, 0x4e, 0x71]);
    Emulator::new(
        rom,
        LoadConfig {
            region,
            concurrent_sound,
        },
    )
    .unwrap()
}

fn compare(actual: &mut Emulator, control: &mut Emulator) {
    assert_eq!(actual, control);
    assert_eq!(actual.timing_snapshot(), control.timing_snapshot());
    assert_eq!(actual.framebuffer(), control.framebuffer());
    assert_eq!(
        actual.framebuffer_dimensions(),
        control.framebuffer_dimensions()
    );
    assert_eq!(
        actual.encode_state().map_err(|e| e.to_string()),
        control.encode_state().map_err(|e| e.to_string())
    );
    let actual_audio: Vec<_> = actual
        .drain_audio_samples()
        .into_iter()
        .map(f32::to_bits)
        .collect();
    let control_audio: Vec<_> = control
        .drain_audio_samples()
        .into_iter()
        .map(f32::to_bits)
        .collect();
    assert_eq!(actual_audio, control_audio);
    assert_eq!(actual.take_vdp_capture(), control.take_vdp_capture());
    assert_eq!(actual, control);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn diagnostic_actual_vdp_write_failure_service_and_candidate_are_observational() {
    for region in [Region::NtscUs, Region::NtscJapan, Region::Pal] {
        for concurrent in [false, true] {
            for word in [0x8100, 0x813c] {
                if region == Region::Pal && word == 0x813c {
                    continue;
                }
                let mut actual = machine(region, concurrent, word);
                let mut control = actual.clone();
                let expected = control.step_field();
                let capture = Capture::begin().unwrap();
                let result = actual.step_field();
                let report = capture.finish(&actual).unwrap();
                assert_eq!(result, expected);
                assert!(
                    matches!(result, Err(crate::MachineFault::Main(TimingFault::Bus {
                    direction: Direction::Write,
                    source: BusError::VdpPort(VdpPortError::Schedule(VdpScheduleError::Register(
                        VdpRegisterError::UnsupportedModeChange { index: 1, value }
                    ))), ..
                })) if value == word as u8)
                );
                assert!(report.complete);
                assert_eq!(report.schema, 2);
                let entry = report.entry.unwrap();
                assert_eq!(entry.snapshot.registers.pc, 0x200);
                assert_eq!(entry.snapshot.queued_words, Some([0x33fc, word]));
                let failed = report.history.iter().flatten().last().unwrap();
                assert_eq!(failed.callback, Callback::Write);
                assert_eq!(failed.access.unwrap().bus_address, 0xc00004);
                assert_eq!(failed.outcome, Some(Outcome::VdpPortError));
                assert_eq!(report.first_failed_sequence, Some(failed.sequence));
                let service = failed.service.unwrap();
                assert!(service.master_ticks > failed.entry.bus.master_ticks);
                let witness = failed.vdp_control.unwrap();
                assert_eq!(witness.word, word);
                assert_eq!(witness.before, service.vdp);
                assert_eq!(witness.before.ticks, service.vdp_ticks);
                assert_eq!(witness.before.registers[1], Some(4));
                assert_eq!(witness.before.read_phase, None);
                assert_eq!(witness.before.fifo_len, 0);
                let rejected = witness.rejection.unwrap();
                assert_eq!(rejected.site, RejectionSite::UnsupportedMode);
                assert_eq!((rejected.index, rejected.value), (1, word as u8));
                assert_eq!(rejected.context, witness.before);
                assert_eq!(rejected.attempted_publish_at, None);
                compare(&mut actual, &mut control);
            }
        }
    }
}

fn access() -> BusAccess {
    BusAccess {
        logical_address: 0xc00004,
        bus_address: 0xc00004,
        function_code: FunctionCode::SupervisorData,
        kind: AccessKind::Data,
        lanes: ByteLanes::Both,
    }
}

fn begin(bus: &Bus, callback: Callback, value: u16) -> u64 {
    begin_call(BusRecord {
        sequence: 0,
        boundary: 0,
        callback,
        access: Some(access()),
        write_value: Some(value),
        requested_start: bus.cpu_clock.0,
        idle_clocks: 0,
        wait_clocks: 0,
        entry: bus.trace_context(Some(0xc00004)),
        service: None,
        vdp_control: None,
        returned: None,
        outcome: None,
    })
    .unwrap()
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn diagnostic_direct_or_pre_service_vdp_calls_cannot_claim_main_write_rejection() {
    let terminal = machine(Region::NtscUs, true, 0x8100);
    let mut bus = Bus::new(
        crate::cartridge::Cartridge::new(terminal.rom_bytes().to_vec()).unwrap(),
        Region::NtscUs,
    );
    for callback in [None, Some(Callback::Read), Some(Callback::Write)] {
        let capture = Capture::begin().unwrap();
        let token = callback.map(|callback| begin(&bus, callback, 0x8100));
        if callback == Some(Callback::Read) {
            service_entry(&bus, access(), Callback::Read);
        }
        assert!(
            bus.vdp
                .write_control_from(
                    0x8100,
                    Region::NtscUs,
                    VdpDmaSource::from_cartridge(&bus.cartridge, &bus.ram)
                )
                .is_err()
        );
        if let Some(sequence) = token {
            end_call(sequence, bus.trace_context(None), 0, Outcome::Complete);
        }
        let report = capture.finish(&terminal).unwrap();
        assert!(report.complete);
        assert!(
            report
                .history
                .iter()
                .flatten()
                .all(|record| record.vdp_control.is_none())
        );
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn diagnostic_control_scope_duplicate_mismatch_and_stale_drop_fail_closed() {
    let terminal = machine(Region::NtscUs, false, 0x8100);
    let bus = Bus::new(
        crate::cartridge::Cartridge::new(terminal.rom_bytes().to_vec()).unwrap(),
        Region::NtscUs,
    );
    for duplicate in [false, true] {
        let capture = Capture::begin().unwrap();
        let token = begin(&bus, Callback::Write, 0x8100);
        service_entry(&bus, access(), Callback::Write);
        let scope = if duplicate {
            let scope = ControlScope::begin(0x8100, bus.vdp.diagnostic_context()).unwrap();
            assert!(ControlScope::begin(0x8100, bus.vdp.diagnostic_context()).is_none());
            Some(scope)
        } else {
            assert!(ControlScope::begin(0x8101, bus.vdp.diagnostic_context()).is_none());
            None
        };
        drop(scope);
        end_call(token, bus.trace_context(None), 0, Outcome::Complete);
        assert!(!capture.finish(&terminal).unwrap().complete);
    }
    let capture = Capture::begin().unwrap();
    begin(&bus, Callback::Write, 0x8100);
    service_entry(&bus, access(), Callback::Write);
    let scope = ControlScope::begin(0x8100, bus.vdp.diagnostic_context()).unwrap();
    drop(capture);
    let fresh = Capture::begin().unwrap();
    drop(scope);
    assert!(fresh.finish(&terminal).unwrap().complete);
}
