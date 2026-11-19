use super::*;
use crate::{ControllerKind, ControllerPort, LoadConfig, Region, TimingModel};
use zeff_emu_common::time::MachineTiming;

mod refusal;
mod slices;
mod tas;

fn write_byte(program: &mut Vec<u16>, address: u32, value: u8) {
    program.extend([
        0x13fc,
        u16::from(value),
        (address >> 16) as u16,
        address as u16,
    ]);
}

pub(super) fn guest(
    region: Region,
    timing: TimingModel,
    concurrent_sound: bool,
    fault_on_b: bool,
) -> Emulator {
    let mut program = Vec::new();
    for (address, value) in [
        (0xc00011, 0x85),
        (0xc00011, 0x06),
        (0xc00011, 0x90),
        (0xa10009, 0x40),
        (0xa1000b, 0x40),
    ] {
        write_byte(&mut program, address, value);
    }
    let loop_pc = 0x200 + program.len() * 2;
    for (level, offset) in [(0x40, 0), (0, 2)] {
        write_byte(&mut program, 0xa10003, level);
        write_byte(&mut program, 0xa10005, level);
        for port in 0..2u16 {
            program.extend([0x1039 | (port << 9), 0x00a1, 3 + port * 2]);
            program.extend([0x13c0 | port, 0x00ff, 0x0100 + offset + port]);
        }
        if fault_on_b && level == 0x40 {
            program.extend([0x0800, 4, 0x6606, 0x1039, 0x00a1, 0x2000]);
        }
    }
    program.extend([0x4ef9, 0, loop_pc as u16]);
    let mut rom = vec![0; 0x4000];
    rom[..8].copy_from_slice(&[0, 0xff, 0xff, 0xfc, 0, 0, 2, 0]);
    for (index, word) in program.into_iter().enumerate() {
        rom[0x200 + index * 2..0x202 + index * 2].copy_from_slice(&word.to_be_bytes());
    }
    let mut core = Emulator::new_with_timing(
        rom,
        LoadConfig {
            region,
            concurrent_sound,
        },
        timing,
    )
    .unwrap();
    core.set_sample_rate(48_000);
    core
}

pub(super) fn lease(core: Emulator) -> MdRollbackSession {
    let config = MdRollbackConfig::capture(&core).unwrap();
    match MdRollbackSession::acquire(Box::new(core), &config) {
        Ok(lease) => lease,
        Err((_, error)) => panic!("{error:#}"),
    }
}

fn pcm(samples: &[f32]) -> Vec<u32> {
    samples.iter().map(|value| value.to_bits()).collect()
}

fn same(actual: &Emulator, expected: &Emulator) {
    assert!(actual == expected, "hardware/runtime equality differs");
    assert_eq!(
        actual.encode_state().unwrap(),
        expected.encode_state().unwrap()
    );
    assert_eq!(
        actual.encode_frontend_state().unwrap(),
        expected.encode_frontend_state().unwrap()
    );
    assert_eq!(
        actual.cpu.queued_instruction_inputs(),
        expected.cpu.queued_instruction_inputs()
    );
    let bytes = actual.rom_bytes().len() as u64;
    assert_eq!(
        actual.bus.capture_board_origins(bytes).unwrap(),
        expected.bus.capture_board_origins(bytes).unwrap()
    );
    assert_eq!(
        actual.bus.vdp.dma_activity_epoch(),
        expected.bus.vdp.dma_activity_epoch()
    );
    assert_eq!(actual.timing_snapshot(), expected.timing_snapshot());
    assert_eq!(actual.cpu_clock(), expected.cpu_clock());
    assert_eq!(actual.pacing_field_count(), expected.pacing_field_count());
    assert_eq!(actual.frame_ready(), expected.frame_ready());
    assert_eq!(actual.framebuffer(), expected.framebuffer());
    assert_eq!(actual.input(), expected.input());
}

fn reference(core: &mut Emulator, raw: [u8; 2]) -> Vec<f32> {
    let mut input = core.input_mut();
    for (port, value) in [ControllerPort::One, ControllerPort::Two]
        .into_iter()
        .zip(raw)
    {
        input.set_controller_raw(port, value);
        input.set_controller_extra_raw(port, 0x0f);
    }
    core.step_field().unwrap();
    core.drain_audio_samples()
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn queued_pcm_real_pad_fields_restore_and_replay() {
    let mut core = guest(Region::NtscUs, TimingModel::Standard, true, false);
    core.step_field().unwrap();
    let baseline = core.clone();
    let mut control = core.clone();
    let baseline_audio = control.drain_audio_samples();
    assert!(baseline_audio.iter().any(|&sample| sample != 0.0));
    let mut owner = lease(core);
    let config = owner.config().clone();
    let saved = owner.capture().unwrap();
    assert!(owner.pending_audio_frames() > 0);
    let first = MdRollbackInputs::new([0x0101, 0x080a], &config).unwrap();
    assert!(owner.advance_frame(first, &config).is_err());
    same(owner.core(), &baseline);
    assert_eq!(
        pcm(&owner.take_pending_audio().unwrap()),
        pcm(&baseline_audio)
    );
    let empty = owner.capture().unwrap();
    let mut outputs = Vec::new();
    for (words, raw, pins) in [
        ([0x0101, 0x080a], [0xe7, 0x5d], [0x67, 0x5d, 0x33, 0x11]),
        ([0x0402, 0x0241], [0xde, 0xab], [0x5e, 0x6b, 0x32, 0x23]),
    ] {
        let inputs = MdRollbackInputs::new(words, &config).unwrap();
        let expected = reference(&mut control, raw);
        let output = owner.advance_frame(inputs, &config).unwrap();
        assert_eq!(output.field, control.pacing_field_count());
        assert_eq!(output.master_tick, control.bus.now.get());
        assert_eq!(
            output.nominal_duration_ns,
            control.nominal_frame_duration_ns()
        );
        assert!(output.audio.iter().any(|&sample| sample != 0.0));
        assert_eq!(pcm(&output.audio), pcm(&expected));
        assert_eq!(&owner.core().work_ram()[0x100..0x104], &pins);
        same(owner.core(), &control);
        outputs.push(pcm(&output.audio));
    }
    let future = control;
    owner.restore(&saved, &config).unwrap();
    same(owner.core(), &baseline);
    assert_eq!(
        pcm(&owner.take_pending_audio().unwrap()),
        pcm(&baseline_audio)
    );
    for (words, expected) in [[0x0101, 0x080a], [0x0402, 0x0241]]
        .into_iter()
        .zip(outputs)
    {
        let output = owner
            .advance_frame(MdRollbackInputs::new(words, &config).unwrap(), &config)
            .unwrap();
        assert_eq!(pcm(&output.audio), expected);
    }
    same(owner.core(), &future);
    owner.restore(&empty, &config).unwrap();
    assert_eq!(owner.pending_audio_frames(), 0);
    let restored = owner.release();
    same(&restored, &baseline);
    assert_eq!(
        pcm(&restored.clone().drain_audio_samples()),
        pcm(&baseline_audio)
    );
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn proposed_profiles_regions_and_mixed_pad_continuations() {
    for region in [Region::NtscUs, Region::NtscJapan, Region::Pal] {
        for (timing, concurrent_sound) in [
            (TimingModel::Standard, false),
            (TimingModel::Standard, true),
            (TimingModel::Precise, true),
        ] {
            for pads in [
                [ControllerKind::ThreeButton; 2],
                [ControllerKind::SixButton; 2],
                [ControllerKind::ThreeButton, ControllerKind::SixButton],
                [ControllerKind::SixButton, ControllerKind::ThreeButton],
            ] {
                let mut core = guest(region, timing, concurrent_sound, false);
                let mut input = core.input_mut();
                for (port, kind) in [ControllerPort::One, ControllerPort::Two]
                    .into_iter()
                    .zip(pads)
                {
                    input.set_controller_kind(port, kind);
                }
                let mut control = core.clone();
                let mut owner = lease(core);
                let config = owner.config().clone();
                let saved = owner.capture().unwrap();
                let hashes = concurrent_sound.then(|| owner.hashes().unwrap());
                if let Some(hashes) = &hashes {
                    assert_eq!(saved.hashes().unwrap(), *hashes);
                    assert_eq!(config.boundary_hashes(owner.core()).unwrap(), *hashes);
                }
                let words = [0x0101, 0x080a];
                let expected = reference(&mut control, [0xe7, 0x5d]);
                let output = owner
                    .advance_frame(MdRollbackInputs::new(words, &config).unwrap(), &config)
                    .unwrap();
                assert_eq!(pcm(&output.audio), pcm(&expected));
                assert!(output.audio.iter().any(|&sample| sample != 0.0));
                if concurrent_sound {
                    let mut into = MdRetainedAllocations::new();
                    owner.visit_retained_allocations(&mut into).unwrap();
                    saved.visit_retained_allocations(&mut into).unwrap();
                    output.visit_retained_allocations(&mut into).unwrap();
                    let cost = into.finish().unwrap();
                    assert_eq!(
                        cost.bytes(MdAllocationKind::FrameAudio),
                        (output.audio.capacity() * size_of::<f32>()) as u64
                    );
                    let mut unbound = owner.core().clone();
                    unbound
                        .decode_frontend_state(&owner.core().encode_frontend_state().unwrap())
                        .unwrap();
                    assert_ne!(
                        config.boundary_hashes(&unbound).unwrap().logical(),
                        owner.hashes().unwrap().logical()
                    );
                }
                same(owner.core(), &control);
                owner.restore(&saved, &config).unwrap();
                if let Some(hashes) = hashes {
                    assert_eq!(owner.hashes().unwrap(), hashes);
                }
                let replay = owner
                    .advance_frame(MdRollbackInputs::new(words, &config).unwrap(), &config)
                    .unwrap();
                assert_eq!(pcm(&replay.audio), pcm(&expected));
                same(owner.core(), &control);
            }
        }
    }
}
