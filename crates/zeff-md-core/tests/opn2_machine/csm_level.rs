use super::cases::{chip_write, grant, machine, write};
use zeff_emu_common::time::MachineTiming;
use zeff_md_core::Region;

const VARIANTS: [(u8, u8, bool); 9] = [
    (0, 0, false),
    (0, 63, false),
    (0, 127, false),
    (0x40, 127, false),
    (0x80, 0, false),
    (0x80, 63, false),
    (0x80, 127, false),
    (0xc0, 127, false),
    (0x80, 127, true),
];

fn program(mode: u8, tl: u8, am: bool, key_mask: u8) -> Vec<u16> {
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
            (0x40, tl),
            (0x50, 31),
            (0x60, if am { 0x80 } else { 0 }),
            (0x70, 0),
            (0x80, 15),
            (0x90, 0),
        ] {
            timed_write(&mut words, group + slot + 2, value);
        }
    }
    // Equal normal/special pitches isolate TL ownership from frequency selection.
    for (register, value) in [
        (0xb2, 7),
        (0xb6, 0xf0),
        (0xa6, 0x22),
        (0xa2, 0x69),
        (0xad, 0x22),
        (0xa9, 0x69),
        (0xae, 0x22),
        (0xaa, 0x69),
        (0xac, 0x22),
        (0xa8, 0x69),
        (0x22, if am { 15 } else { 0 }),
        (0x27, mode),
        (0x28, key_mask | 2),
    ] {
        timed_write(&mut words, register, value);
    }
    words.extend([0x60fe, 0x4e71]);
    words
}

pub fn csm_mode_omits_ordinary_total_level_and_retains_am() {
    for region in [Region::NtscUs, Region::Pal, Region::NtscJapan] {
        for concurrent in [false, true] {
            for key_mask in [0x10, 0x20, 0x40, 0x80] {
                let mut captures = Vec::new();
                for (mode, tl, am) in VARIANTS {
                    let mut source = machine(&program(mode, tl, am, key_mask), region, concurrent);
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
                    let context = format!(
                        "{region:?}, concurrent={concurrent}, key={key_mask:#x}, mode={mode:#x}, TL={tl}, AM={am}"
                    );
                    assert!(pcm.len() > 4_000, "missing PCM: {context}");
                    assert!(pcm.iter().all(|sample| sample.is_finite()), "{context}");
                    assert!(
                        pcm.as_chunks::<2>()
                            .0
                            .iter()
                            .all(|frame| frame[0] == frame[1]),
                        "stereo routing: {context}"
                    );
                    if mode == 0x80 || tl == 0 {
                        assert!(
                            pcm.iter().any(|sample| sample.abs() > 0.01),
                            "missing keyed output: {context}"
                        );
                    }
                    assert!(source.video_error().is_none(), "{context}");
                    assert!(source.sound_fault().is_none(), "{context}");
                    assert_eq!(source.sound_cpu().cycles(), 0, "{context}");
                    assert!(source.sound_bus_requested() && source.sound_bus_granted());
                    assert!(!source.sound_reset_asserted());
                    let snapshot = source.opn2_snapshot();
                    assert_eq!(snapshot.mode, mode, "{context}");
                    assert_eq!(snapshot.status & 3, 0, "{context}");
                    assert_eq!((snapshot.timer_a, snapshot.timer_b), (0, 0), "{context}");
                    captures.push(pcm);
                }
                let context = format!("{region:?}, concurrent={concurrent}, key={key_mask:#x}");
                for index in [4, 5, 6] {
                    assert!(
                        captures[0] == captures[index],
                        "CSM retained ordinary TL{}: {context}",
                        VARIANTS[index].1
                    );
                }
                for (a, b) in [(0, 1), (0, 2), (1, 2)] {
                    assert!(
                        captures[a] != captures[b],
                        "ordinary TL controls collapsed: {a}/{b}, {context}"
                    );
                }
                for index in [3, 7] {
                    assert!(
                        captures[2] == captures[index],
                        "mode {:#x} omitted ordinary TL: {context}",
                        VARIANTS[index].0
                    );
                }
                assert!(captures[6] != captures[8], "CSM lost AM: {context}");
            }
        }
    }
}
