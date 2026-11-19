use super::cases::{chip_write, grant, machine, write};
use zeff_emu_common::time::MachineTiming;
use zeff_md_core::Region;

#[derive(Clone, Copy, Debug)]
struct Case {
    mode: u8,
    period: u16,
    tl: u8,
    flag_enabled: bool,
    manual: u8,
}

impl Case {
    fn control(self) -> u8 {
        self.mode | 1 | if self.flag_enabled { 4 } else { 0 }
    }
}

fn program(case: Case) -> Vec<u16> {
    fn timed_write(words: &mut Vec<u16>, register: u8, value: u8) {
        chip_write(words, register, value);
        words.extend(std::iter::repeat_n(0x4e71, 64));
    }

    let mut words = vec![0x46fc, 0x2700];
    grant(&mut words);
    words.extend([0x33fc, 0x8104, 0x00c0, 0x0004]);
    for value in [0x9f, 0xbf, 0xdf, 0xff] {
        write(&mut words, 0xc00011, value);
    }
    for slot in [0, 4, 8, 12] {
        for (group, value) in [
            (0x30, 1),
            (0x40, case.tl),
            (0x50, 31),
            (0x60, 0),
            (0x70, 0),
            (0x80, 15),
            (0x90, 0),
        ] {
            timed_write(&mut words, group + slot + 2, value);
        }
    }
    // Equal normal/special pitches isolate CSM key and envelope ownership.
    for (register, value) in [
        (0xb2, 7),
        (0xb6, 0xc0),
        (0xa6, 0x22),
        (0xa2, 0x69),
        (0xad, 0x22),
        (0xa9, 0x69),
        (0xae, 0x22),
        (0xaa, 0x69),
        (0xac, 0x22),
        (0xa8, 0x69),
        (0x22, 0),
        (0x26, 0),
        (0x27, 0x30),
        (0x28, 2),
    ] {
        timed_write(&mut words, register, value);
    }
    let reload = 1024 - case.period;
    timed_write(&mut words, 0x24, (reload >> 2) as u8);
    timed_write(&mut words, 0x25, (reload & 3) as u8);
    timed_write(&mut words, 0x28, case.manual | 2);
    timed_write(&mut words, 0x27, case.control());
    write(&mut words, 0xff0100, 1);
    words.extend([0x60fe, 0x4e71]);
    words
}

fn capture(case: Case, region: Region, concurrent: bool) -> Vec<f32> {
    let mut source = machine(&program(case), region, concurrent);
    let mut pcm = Vec::new();
    for field in 1..=6 {
        while source.timing_snapshot().now().get() < field * region.frame_ticks() {
            source.step_instruction().unwrap();
        }
        if field <= 3 {
            source.drain_audio_samples();
        } else {
            source.drain_audio_samples_into(&mut pcm);
        }
    }
    let context = format!("{case:?}, {region:?}, concurrent={concurrent}");
    assert_eq!(source.peek(0xff0100), Some(1), "setup: {context}");
    assert!(pcm.len() > 4_000, "missing PCM: {context}");
    assert!(pcm.iter().all(|sample| sample.is_finite()), "{context}");
    assert!(
        pcm.as_chunks::<2>()
            .0
            .iter()
            .all(|frame| frame[0] == frame[1]),
        "stereo routing: {context}"
    );
    let low = pcm.iter().copied().fold(f32::INFINITY, f32::min);
    let high = pcm.iter().copied().fold(f32::NEG_INFINITY, f32::max);
    if case.manual != 0 || (case.mode == 0x80 && case.period == 1) {
        assert!(
            high - low > 0.00001,
            "missing keyed waveform {low}..{high}: {context}"
        );
    } else if case.mode == 0x80 && case.tl == 0 {
        assert!(
            high.abs().max(low.abs()) > 0.00001,
            "missing keyed output {low}..{high}: {context}"
        );
    } else if case.mode != 0x80 {
        assert_eq!(high, low, "unexpected automatic key: {context}");
    }
    assert!(source.video_error().is_none(), "{context}");
    assert!(source.sound_fault().is_none(), "{context}");
    assert_eq!(source.sound_cpu().cycles(), 0, "{context}");
    assert!(source.sound_bus_requested() && source.sound_bus_granted());
    assert!(!source.sound_reset_asserted());
    let snapshot = source.opn2_snapshot();
    assert_eq!(snapshot.mode, case.control(), "{context}");
    assert_eq!(snapshot.timer_a, 1024 - case.period, "{context}");
    assert_eq!(snapshot.timer_b, 0, "{context}");
    assert_eq!(
        snapshot.status & 3,
        u8::from(case.flag_enabled),
        "{context}"
    );
    pcm
}

pub fn timer_a_csm_keys_transfer_level_without_flag_enable() {
    for region in [Region::NtscUs, Region::Pal, Region::NtscJapan] {
        for concurrent in [false, true] {
            let context = format!("{region:?}, concurrent={concurrent}");
            let mut automatic = Vec::new();
            for tl in [0, 63] {
                for period in [1, 2] {
                    let mut silent = Vec::new();
                    for mode in [0, 0x40, 0x80, 0xc0] {
                        let case = Case {
                            mode,
                            period,
                            tl,
                            flag_enabled: false,
                            manual: 0,
                        };
                        let disabled = capture(case, region, concurrent);
                        let enabled = capture(
                            Case {
                                flag_enabled: true,
                                ..case
                            },
                            region,
                            concurrent,
                        );
                        assert!(
                            disabled == enabled,
                            "flag enable changed PCM: {case:?}, {context}"
                        );
                        if mode == 0x80 {
                            automatic.push(enabled);
                        } else {
                            silent.push(enabled);
                        }
                    }
                    assert!(silent[0] == silent[1] && silent[0] == silent[2]);
                }
            }
            for (a, b) in [(0, 1), (2, 3)] {
                assert!(
                    automatic[a] != automatic[b],
                    "continuous/separated CSM keys collapsed: {a}/{b}, {context}"
                );
            }
            for (a, b) in [(0, 2), (1, 3)] {
                assert!(
                    automatic[a] != automatic[b],
                    "CSM envelope TL floor absent: {a}/{b}, {context}"
                );
            }
            for (tl, index) in [(0, 1), (63, 3)] {
                let manual = capture(
                    Case {
                        mode: 0x80,
                        period: 2,
                        tl,
                        flag_enabled: true,
                        manual: 0xf0,
                    },
                    region,
                    concurrent,
                );
                assert!(
                    manual != automatic[index],
                    "manual key failed to suppress CSM retrigger: TL={tl}, {context}"
                );
            }
        }
    }
}
