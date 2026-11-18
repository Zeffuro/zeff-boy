use zeff_md_core::{Emulator, LoadConfig, Region};

fn image(words: &[u16]) -> Vec<u8> {
    let mut bytes = vec![0; 0x4000];
    bytes[..8].copy_from_slice(&[0, 0xff, 0xff, 0xfc, 0, 0, 1, 0]);
    for (index, word) in words.iter().enumerate() {
        bytes[0x100 + 2 * index..0x102 + 2 * index].copy_from_slice(&word.to_be_bytes());
    }
    bytes
}

fn program(h40: bool, consume: bool) -> Vec<u16> {
    let mut words = vec![
        0x33fc,
        if h40 { 0x8c81 } else { 0x8c00 },
        0x00c0,
        4,
        0x33fc,
        0x8144,
        0x00c0,
        4,
        0x33fc,
        0x8f01,
        0x00c0,
        4,
        0x23fc,
        0x0000,
        0x8f02,
        0x00c0,
        4,
    ];
    words.extend(std::iter::repeat_n(0x4e71, 8));
    if consume {
        words.extend([0x3039, 0x00c0, 0]);
    } else {
        words.extend([
            0x33fc, 0x8f02, 0x00c0, 4, 0x23fc, 0x4000, 0, 0x00c0, 4, 0x33fc, 0xa357, 0x00c0, 0,
            0x23fc, 0, 0, 0x00c0, 4, 0x3039, 0x00c0, 0,
        ]);
    }
    words.push(0x60fe);
    words
}

pub fn main_bus_restore_and_cancellation() {
    for region in [Region::NtscUs, Region::NtscJapan, Region::Pal] {
        for h40 in [false, true] {
            for concurrent_sound in [false, true] {
                let mut machine = Emulator::new(
                    image(&program(h40, false)),
                    LoadConfig {
                        region,
                        concurrent_sound,
                    },
                )
                .unwrap();
                let mut saw_ready_odd = false;
                for _ in 0..32 {
                    let bytes = machine.encode_state().unwrap();
                    assert_eq!(&bytes[8..12], &1_u32.to_le_bytes());
                    let mut restored = machine.clone();
                    restored.decode_state(&bytes).unwrap();
                    assert_eq!(bytes, restored.encode_state().unwrap());
                    assert_eq!(machine.step_instruction(), restored.step_instruction());
                    assert_eq!(
                        machine.encode_state().unwrap(),
                        restored.encode_state().unwrap()
                    );
                    let vdp = machine.vdp();
                    if vdp.registers().auto_increment() == Some(1)
                        && vdp.command().code == 0
                        && vdp.command().address == 0x8001
                    {
                        assert!(vdp.read_cache().is_ready());
                        saw_ready_odd = true;
                    }
                }
                assert!(saw_ready_odd);
                assert_eq!(machine.registers().d[0] & 0xffff, 0xa357);
                assert_eq!(&machine.vdp().memory().vram()[..2], &[0xa3, 0x57]);
                machine.drain_audio_samples();
                let bytes = machine.encode_frontend_state().unwrap();
                let mut restored = machine.clone();
                restored.decode_frontend_state(&bytes).unwrap();
                machine.step_field().unwrap();
                restored.step_field().unwrap();
                assert_eq!(
                    machine.encode_frontend_state().unwrap(),
                    restored.encode_frontend_state().unwrap()
                );
                assert!(machine.framebuffer() == restored.framebuffer());
                assert!(
                    machine.drain_audio_samples() == restored.drain_audio_samples(),
                    "restored future PCM"
                );
            }
        }
    }
}

pub fn main_bus_odd_refill_continuation() {
    for region in [Region::NtscUs, Region::NtscJapan, Region::Pal] {
        let mut words = program(false, true);
        words.splice(
            12..12,
            [0x23fc, 0x4000, 2, 0x00c0, 4, 0x33fc, 0xa357, 0x00c0, 0],
        );
        words.splice(
            words.len() - 4..words.len() - 4,
            std::iter::repeat_n(0x4e71, 128),
        );
        let mut machine = Emulator::new(
            image(&words),
            LoadConfig {
                region,
                concurrent_sound: true,
            },
        )
        .unwrap();
        for _ in 0..142 {
            machine.step_instruction().unwrap();
        }
        let command = machine.vdp().command();
        let memory = machine.vdp().memory().clone();
        let fifo = machine.vdp().fifo().clone();
        assert_eq!(command.address, 0x8001);
        assert!(machine.vdp().read_cache().is_ready());
        machine.step_instruction().unwrap();
        assert_eq!(machine.registers().d[0] & 0xffff, 0xa357);
        let saved = machine.encode_state().unwrap();
        let mut restored = machine.clone();
        restored.decode_state(&saved).unwrap();
        for _ in 0..32 {
            assert_eq!(machine.step_instruction(), restored.step_instruction());
            assert_eq!(
                machine.encode_state().unwrap(),
                restored.encode_state().unwrap()
            );
        }
        assert_eq!(machine.vdp().command().address, 0x8002);
        assert_eq!(machine.vdp().read_cache().buffered_word(), Some(0xa357));
        assert!(machine.vdp().read_cache().is_ready());
        assert_eq!(machine.vdp().memory(), &memory);
        assert_eq!(machine.vdp().fifo(), &fifo);
        assert!(!machine.main_cpu_stopped());
    }
}
