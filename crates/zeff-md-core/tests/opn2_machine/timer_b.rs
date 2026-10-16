use super::cases::{chip_write, grant, machine, write};
use zeff_emu_common::time::MachineTiming;
use zeff_md_core::{Emulator, Region};

struct Program {
    words: Vec<u16>,
    cuts: Vec<u32>,
}

impl Program {
    fn timer(&mut self, register: u8, value: u8, cut: bool) {
        chip_write(&mut self.words, register, value);
        if cut {
            self.cuts.push(0x100 + self.words.len() as u32 * 2);
        }
    }

    fn wait(&mut self, nops: usize) {
        self.words.extend(std::iter::repeat_n(0x4e71, nops));
    }

    fn status(&mut self) {
        self.words.extend([0x1239, 0x00a0, 0x4000, 0x0201, 3]);
    }

    fn stage(&mut self, stage: u8, poll: Option<u8>) {
        if let Some(expected) = poll {
            self.words.push(0x7000);
            let start = self.words.len();
            self.words.push(0x5240);
            self.status();
            self.words.extend([0x0c01, u16::from(expected)]);
            let branch = self.words.len();
            let displacement = (start as isize - branch as isize - 1) * 2;
            self.words
                .push(0x6600 | u16::from(displacement as i8 as u8));
            self.words
                .extend([0x33c0, 0x00ff, 0x0140 + u16::from(stage) * 2]);
        } else {
            self.status();
        }
        self.words
            .extend([0x13c1, 0x00ff, 0x0120 + u16::from(stage)]);
        write(&mut self.words, 0xff0100, stage);
    }
}

fn program(stopped_wait: usize) -> Program {
    let mut program = Program {
        words: vec![0x46fc, 0x2700],
        cuts: Vec::new(),
    };
    grant(&mut program.words);
    program.timer(0x2a, 0x91, false);
    program.timer(0x2b, 0x80, false);
    program.timer(0x26, 255, false);
    program.wait(64 + stopped_wait);
    program.stage(1, None);
    program.timer(0x27, 0x0a, true);
    program.stage(2, Some(2));
    program.timer(0x27, 0x22, true);
    program.wait(1024);
    program.stage(3, None);
    program.timer(0x27, 0x0a, true);
    program.stage(4, Some(2));
    program.timer(0x27, 0x28, true);
    program.wait(256);
    program.stage(5, None);
    program.timer(0x27, 0x0a, true);
    program.stage(6, Some(2));
    program.timer(0x26, 253, true);
    program.timer(0x27, 0x2a, true);
    program.stage(7, Some(2));
    program.timer(0x24, 255, false);
    program.timer(0x25, 3, false);
    program.timer(0x27, 0x0f, true);
    program.stage(8, Some(3));
    program.timer(0x27, 0x1a, true);
    program.stage(9, Some(2));
    program.timer(0x27, 0x30, true);
    program.wait(64);
    program.stage(10, None);
    write(&mut program.words, 0xff0101, 1);
    program.words.extend([0x60fe, 0x4e71]);
    program
}

fn saved_future(source: &mut Emulator, program: &Program, region: Region, concurrent: bool) {
    source.drain_audio_samples();
    let saved = source.encode_state().unwrap();
    let mut restored = machine(&program.words, region, concurrent);
    restored.decode_state(&saved).unwrap();
    assert_eq!(restored.encode_state().unwrap(), saved);
    let mut expected = source.clone();
    // Canonical restore restarts host resampling while preserving chip state.
    expected.set_sample_rate(expected.sample_rate());
    for _ in 0..192 {
        assert_eq!(expected.step_instruction(), restored.step_instruction());
    }
    assert_eq!(
        expected.encode_state().unwrap(),
        restored.encode_state().unwrap()
    );
    let pcm = expected.drain_audio_samples();
    assert_eq!(pcm, restored.drain_audio_samples());
    assert!(pcm.iter().all(|sample| sample.is_finite()));
}

fn capture(program: &Program, region: Region, concurrent: bool) -> u16 {
    let mut source = machine(&program.words, region, concurrent);
    let initial_sound = source.sound_cpu().clone();
    let mut cuts = 0;
    let mut stages = 0;
    let mut audible = false;
    let mut completed = false;
    for _ in 0..20_000 {
        source.step_instruction().unwrap();
        assert_eq!(source.sound_cpu(), &initial_sound);
        if program.cuts.contains(&source.registers().pc) {
            saved_future(&mut source, program, region, concurrent);
            cuts += 1;
        }
        let stage = source.peek(0xff0100).unwrap();
        if stage != stages {
            assert_eq!(stage, stages + 1);
            let expected = [0, 2, 0, 2, 0, 2, 2, 3, 2, 0][stage as usize - 1];
            assert_eq!(source.peek(0xff0120 + u32::from(stage)), Some(expected));
            assert_eq!(source.opn2_snapshot().status & 3, expected);
            stages = stage;
        }
        audible |= source
            .drain_audio_samples()
            .iter()
            .any(|sample| *sample != 0.0);
        if source.peek(0xff0101) == Some(1) {
            completed = true;
            break;
        }
    }
    let context = format!("{region:?}, concurrent={concurrent}");
    assert!(completed, "Timer B polling did not complete: {context}");
    assert_eq!(stages, 10, "{context}");
    assert_eq!(cuts, program.cuts.len(), "{context}");
    assert!(audible, "missing DAC control PCM: {context}");
    assert!(source.video_error().is_none(), "{context}");
    assert!(source.sound_fault().is_none(), "{context}");
    assert!(source.sound_bus_requested() && source.sound_bus_granted());
    assert!(!source.sound_reset_asserted());
    assert_eq!(source.opn2_snapshot().mode, 0);
    assert_eq!(source.opn2_snapshot().timer_b, 253);
    assert!(source.timing_snapshot().now().get() < region.frame_ticks());
    let high = source.peek(0xff0144).unwrap();
    let low = source.peek(0xff0145).unwrap();
    let polls = u16::from_be_bytes([high, low]);
    assert!(
        (1..=64).contains(&polls),
        "first B poll count {polls}: {context}"
    );
    polls
}

pub fn timer_b_polling_retains_stopped_phase_and_saved_future() {
    let early = program(0);
    let late = program(256);
    for region in [Region::NtscUs, Region::Pal, Region::NtscJapan] {
        for concurrent in [false, true] {
            let first = capture(&early, region, concurrent);
            let shifted = capture(&late, region, concurrent);
            // A start-relative divider would give the same polling interval.
            assert!(
                first.abs_diff(shifted) >= 8,
                "{region:?}, concurrent={concurrent}: {first}/{shifted}"
            );
        }
    }
}
