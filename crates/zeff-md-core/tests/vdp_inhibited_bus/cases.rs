use zeff_md_core::{Emulator, LoadConfig, Region};

fn image(h40: bool) -> Vec<u8> {
    let mut words = Vec::new();
    let mut word = |address: u16, value: u16| {
        words.extend([0x33fc, value, 0x00c0, address]);
    };
    word(4, if h40 { 0x8c81 } else { 0x8c00 });
    word(4, 0x8134);
    word(4, 0x8f02);
    word(4, 0x5000);
    word(4, 0);
    word(0, 0xa357);
    word(4, 0);
    word(4, 0x0050);
    word(4, 0);
    word(0, 0xdead);
    word(0, 0xbeef);
    word(4, 0x5002);
    word(4, 0);
    word(0, 0x2468);
    words.extend([0x4e71; 8]);
    words.push(0x60fe);
    let mut bytes = vec![0; 0x4000];
    bytes[..8].copy_from_slice(&[0, 0xff, 0xff, 0xfc, 0, 0, 1, 0]);
    for (index, value) in words.into_iter().enumerate() {
        bytes[0x100 + 2 * index..0x102 + 2 * index].copy_from_slice(&value.to_be_bytes());
    }
    bytes
}

pub fn main_bus_ignored_write_restore() {
    for region in [Region::NtscUs, Region::NtscJapan, Region::Pal] {
        for h40 in [false, true] {
            for concurrent_sound in [false, true] {
                let mut machine = Emulator::new(
                    image(h40),
                    LoadConfig {
                        region,
                        concurrent_sound,
                    },
                )
                .unwrap();
                let mut ignored = 0;
                for _ in 0..24 {
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
                    if vdp.command().code == 0x14 && vdp.control().pending_word().is_none() {
                        match vdp.command().address {
                            2 => ignored |= 1,
                            4 => ignored |= 2,
                            _ => {}
                        }
                    }
                }
                assert_eq!(ignored, 3);
                assert!(machine.vdp().fifo().is_empty());
                let memory = machine.vdp().memory();
                assert_eq!(&memory.vram()[0x1000..0x1004], &[0xa3, 0x57, 0x24, 0x68]);
                assert!(memory.vram()[..0x1000].iter().all(|&byte| byte == 0));
                assert!(memory.vram()[0x1004..].iter().all(|&byte| byte == 0));
                assert!(memory.known_cram().unwrap().iter().all(|&value| value == 0));
                assert!(memory.vsram().iter().all(|&value| value == 0));
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
                assert_eq!(machine.framebuffer(), restored.framebuffer());
                assert_eq!(
                    machine.drain_audio_samples(),
                    restored.drain_audio_samples()
                );
            }
        }
    }
}
