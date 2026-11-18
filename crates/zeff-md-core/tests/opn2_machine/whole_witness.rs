use super::cases::{chip_write, grant, machine, write};
use sha2::{Digest, Sha256};
use zeff_emu_common::time::MachineTiming;
use zeff_md_core::{Emulator, LoadConfig, Region};

#[derive(Default)]
struct Witness {
    hash: Sha256,
    bytes: u64,
    cuts: u64,
    futures: u64,
    samples: u64,
    nonzero: u64,
    statuses: u64,
    flags: u8,
}

impl Witness {
    fn record(&mut self, tag: u8, bytes: &[u8]) {
        self.hash.update([tag]);
        self.hash.update((bytes.len() as u64).to_le_bytes());
        self.hash.update(bytes);
        self.bytes += 9 + bytes.len() as u64;
    }

    fn numbers(&mut self, tag: u8, values: &[u64]) {
        let bytes: Vec<_> = values.iter().flat_map(|x| x.to_le_bytes()).collect();
        self.record(tag, &bytes);
    }

    fn status(&mut self, machine: &Emulator) {
        let status = machine.peek(0xa04000).unwrap();
        self.flags |= status & 3;
        self.numbers(
            5,
            &[
                machine.timing_snapshot().now().get(),
                machine.registers().pc as u64,
                u64::from(status),
            ],
        );
        self.statuses += 1;
    }

    fn pcm(&mut self, samples: &[f32]) {
        assert_eq!(samples.len() % 2, 0);
        assert!(samples.iter().all(|x| x.is_finite()));
        let bytes: Vec<_> = samples
            .iter()
            .flat_map(|x| x.to_bits().to_le_bytes())
            .collect();
        self.record(6, &bytes);
        self.samples += samples.len() as u64;
        self.nonzero += samples.iter().filter(|x| **x != 0.0).count() as u64;
    }
}

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
        self.words.extend(std::iter::repeat_n(0x4e71, 64));
    }

    fn wait(&mut self) {
        self.words.extend(std::iter::repeat_n(0x4e71, 1024));
    }
}

fn program(period: u16, manual: u8) -> Program {
    let mut p = Program {
        words: vec![0x46fc, 0x2700],
        cuts: Vec::new(),
    };
    grant(&mut p.words);
    p.words.extend([0x33fc, 0x8104, 0x00c0, 0x0004]);
    for value in [0x9f, 0xbf, 0xdf, 0xff] {
        write(&mut p.words, 0xc00011, value);
    }
    for slot in [0, 4, 8, 12] {
        for (group, value) in [
            (0x30, 1),
            (0x40, 0),
            (0x50, 31),
            (0x60, 0),
            (0x70, 0),
            (0x80, 15),
            (0x90, 0),
        ] {
            p.timer(group + slot + 2, value, false);
        }
    }
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
        (0x26, 255),
    ] {
        p.timer(register, value, false);
    }
    let reload = 1024 - period;
    p.timer(0x24, (reload >> 2) as u8, false);
    p.timer(0x25, (reload & 3) as u8, false);
    p.timer(0x28, manual | 2, false);
    p.timer(0x27, 0x8f, true);
    p.wait();
    p.timer(0x27, 0xbf, true);
    p.timer(0x24, 255, true);
    p.timer(0x25, (reload.wrapping_sub(1) & 3) as u8, true);
    p.timer(0x28, manual ^ 0xf0 | 2, true);
    p.wait();
    p.timer(0x27, 0x0f, true);
    p.timer(0x27, 0x30, true);
    write(&mut p.words, 0xff0100, 1);
    p.words.extend([0x60fe, 0x4e71]);
    p
}

fn future(
    w: &mut Witness,
    source: &Emulator,
    p: &Program,
    settings: LoadConfig,
    frontend: bool,
    instructions: u64,
) {
    let raw = source.encode_state().unwrap();
    let front = source.encode_frontend_state().unwrap();
    assert_eq!(&raw[..8], b"ZMDRAW01");
    assert_eq!(u32::from_le_bytes(raw[8..12].try_into().unwrap()), 1);
    assert_eq!(&front[..8], b"ZMDFRONT");
    assert_eq!(u32::from_le_bytes(front[8..12].try_into().unwrap()), 1);
    let mut restored = machine(&p.words, settings.region, settings.concurrent_sound);
    let mut expected = source.clone();
    if frontend {
        restored.decode_frontend_state(&front).unwrap();
    } else {
        restored.decode_state(&raw).unwrap();
        expected.decode_state(&raw).unwrap();
    }
    assert_eq!(restored.encode_state().unwrap(), raw);
    assert_eq!(
        restored.encode_frontend_state().unwrap(),
        expected.encode_frontend_state().unwrap()
    );
    let before_raw = restored.encode_state().unwrap();
    let before_front = restored.encode_frontend_state().unwrap();
    for frontend in [false, true] {
        let result = if frontend {
            restored.decode_frontend_state(&front[..12])
        } else {
            restored.decode_state(&raw[..12])
        };
        assert!(result.is_err());
        assert_eq!(restored.encode_state().unwrap(), before_raw);
        assert_eq!(restored.encode_frontend_state().unwrap(), before_front);
    }
    w.numbers(3, &[u64::from(frontend), instructions]);
    w.status(&restored);
    for _ in 0..instructions {
        assert_eq!(
            expected.step_instruction().unwrap(),
            restored.step_instruction().unwrap()
        );
        assert_eq!(expected.peek(0xa04000), restored.peek(0xa04000));
        w.status(&restored);
    }
    assert_eq!(expected.timing_snapshot(), restored.timing_snapshot());
    assert_eq!(
        expected.encode_state().unwrap(),
        restored.encode_state().unwrap()
    );
    assert_eq!(
        expected.encode_frontend_state().unwrap(),
        restored.encode_frontend_state().unwrap()
    );
    let pcm = expected.drain_audio_samples();
    let other = restored.drain_audio_samples();
    assert_eq!(pcm.len(), other.len());
    assert!(
        pcm.iter()
            .zip(&other)
            .all(|(a, b)| a.to_bits() == b.to_bits())
    );
    w.pcm(&pcm);
    w.record(7, &restored.encode_state().unwrap());
    w.record(8, &restored.encode_frontend_state().unwrap());
    assert!(restored.video_error().is_none() && restored.sound_fault().is_none());
    w.futures += 1;
}

pub fn raw_frontend_active_csm_manual_reload_clear_future_witness() {
    let mut w = Witness::default();
    w.record(
        0,
        b"machine-opn2-timera-v1/raw20-frontend1/le/tag-u8-length-u64",
    );
    for (region_id, region) in [Region::NtscUs, Region::NtscJapan, Region::Pal]
        .into_iter()
        .enumerate()
    {
        for concurrent in [false, true] {
            for period in [1u16, 2] {
                for manual in [0, 0xf0] {
                    let p = program(period, manual);
                    let mut source = machine(&p.words, region, concurrent);
                    let nonzero_before = w.nonzero;
                    let mut next = 0;
                    for _ in 0..20_000 {
                        source.step_instruction().unwrap();
                        if next < p.cuts.len() && source.registers().pc == p.cuts[next] {
                            source.drain_audio_samples();
                            w.numbers(
                                1,
                                &[
                                    region_id as u64,
                                    u64::from(concurrent),
                                    u64::from(period),
                                    u64::from(manual),
                                    next as u64,
                                    source.timing_snapshot().now().get(),
                                ],
                            );
                            w.record(2, &source.encode_state().unwrap());
                            w.record(4, &source.encode_frontend_state().unwrap());
                            for frontend in [false, true] {
                                for instructions in [128, 2048] {
                                    future(
                                        &mut w,
                                        &source,
                                        &p,
                                        LoadConfig {
                                            region,
                                            concurrent_sound: concurrent,
                                        },
                                        frontend,
                                        instructions,
                                    );
                                }
                            }
                            next += 1;
                            w.cuts += 1;
                        }
                        if source.peek(0xff0100) == Some(1) {
                            break;
                        }
                    }
                    assert_eq!(next, p.cuts.len());
                    assert_eq!(source.peek(0xff0100), Some(1));
                    assert!(w.nonzero > nonzero_before);
                }
            }
        }
    }
    assert_eq!(w.cuts, 3 * 2 * 2 * 2 * 7);
    assert_eq!(w.futures, w.cuts * 4);
    assert_eq!(w.flags, 3);
    let digest: String = w
        .hash
        .finalize()
        .iter()
        .map(|x| format!("{x:02x}"))
        .collect();
    let line = format!(
        "MACHINE_OPN2_WITNESS {{\"schema\":1,\"regions\":3,\"concurrentModes\":2,\"periods\":2,\"manualModes\":2,\"cuts\":{},\"futures\":{},\"pcmSamples\":{},\"nonzeroSamples\":{},\"statusReads\":{},\"flagsSeen\":{},\"hashedBytes\":{},\"sha256\":\"{digest}\"}}",
        w.cuts, w.futures, w.samples, w.nonzero, w.statuses, w.flags, w.bytes
    );
    #[cfg(not(target_arch = "wasm32"))]
    println!("{line}");
    #[cfg(target_arch = "wasm32")]
    wasm_bindgen_test::console_log!("{}", line);
}
