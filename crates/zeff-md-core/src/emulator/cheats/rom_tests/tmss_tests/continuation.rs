use super::*;
use sha2::{Digest, Sha256};

fn decode(machine: &mut Emulator, bytes: &[u8], codec: usize) -> anyhow::Result<()> {
    match codec {
        0 => machine.decode_state(bytes),
        1 => machine.decode_frontend_state(bytes),
        2 => machine.decode_debugger_checkpoint(bytes),
        _ => unreachable!(),
    }
}

fn late_corruption(saved: &[Vec<u8>; 3], codec: usize) -> Vec<u8> {
    let mut raw = saved[0].clone();
    let tag = raw
        .windows(8)
        .position(|bytes| bytes == b"ZMDTMSS1")
        .unwrap();
    let payload = tag + 12;
    assert_eq!(
        u32::from_le_bytes(raw[tag + 8..payload].try_into().unwrap()),
        37
    );
    raw[payload + 36] = 2;
    match codec {
        0 => raw,
        1 => {
            let mut front = saved[1].clone();
            assert_eq!(&front[16..16 + raw.len()], saved[0]);
            front[16..16 + raw.len()].copy_from_slice(&raw);
            front
        }
        2 => {
            let mut owned = saved[2].clone();
            let mut front = saved[1].clone();
            front[16..16 + raw.len()].copy_from_slice(&raw);
            assert_eq!(&owned[80..80 + front.len()], saved[1]);
            owned[80..80 + front.len()].copy_from_slice(&front);
            owned[44..76].copy_from_slice(&Sha256::digest(&front));
            let end = owned.len() - 32;
            let digest = Sha256::digest(&owned[..end]);
            owned[end..].copy_from_slice(&digest);
            owned
        }
        _ => unreachable!(),
    }
}

fn failure_preserves(receiver: &mut Emulator, bytes: &[u8], codec: usize) {
    let before = receiver.clone();
    let encoded = states(receiver);
    let overlay = receiver.bus.cartridge.rom_overlay.clone();
    assert!(decode(receiver, bytes, codec).is_err());
    assert_eq!(*receiver, before);
    assert_eq!(states(receiver), encoded);
    assert_eq!(receiver.bus.cartridge.rom_overlay, overlay);
    assert_eq!(receiver.framebuffer(), before.framebuffer());
    assert_eq!(
        receiver.clone().drain_audio_samples(),
        before.clone().drain_audio_samples()
    );
}

pub(super) fn check(region: Region, concurrent: bool) {
    let program = [0x7001, 0x0640, 3, 0x60f8, 0x4e71];
    let set_a = patches("010100:7007 + 018000:A55A");
    let mut source = machine(&program, region, concurrent);
    source.install_rom_patches(&set_a).unwrap();
    for boot_mapped in [true, false] {
        if !boot_mapped {
            enter_cartridge(&mut source);
        }
        let saved = states(&source);
        for (set, opcode, data) in [
            ("010100:7007 + 018000:A55A", 7, 0xa5),
            ("010100:7009 + 018000:5AA5", 9, 0x5a),
            ("", 1, 0x12),
        ] {
            for codec in 0..3 {
                let mut receiver = machine(&program, region, concurrent);
                receiver.install_rom_patches(&patches(set)).unwrap();
                let overlay = receiver.bus.cartridge.rom_overlay.clone();
                enter_cartridge(&mut receiver);
                receiver.step_instruction().unwrap();
                receiver.debug_write_ram(&[(0xff0040, 0x55)]).unwrap();
                for bad in [
                    saved[codec][..saved[codec].len() - 1].to_vec(),
                    [saved[codec].as_slice(), &[0]].concat(),
                    late_corruption(&saved, codec),
                ] {
                    failure_preserves(&mut receiver, &bad, codec);
                }
                restore(&mut receiver, &saved, codec);
                assert_eq!(receiver.bus.cartridge.rom_overlay, overlay);
                assert_eq!(
                    receiver.bus.tmss.as_ref().unwrap().boot_mapped(),
                    boot_mapped
                );
                if boot_mapped {
                    assert_eq!(receiver.rom_offset_for_cpu_address(CODE), None);
                    enter_cartridge(&mut receiver);
                    assert_eq!(receiver.queued_words(), Some([0x7000 | opcode, 0x0640]));
                } else {
                    assert_eq!(receiver.queued_words(), Some([0x7007, 0x0640]));
                }
                assert_eq!(receiver.peek(DATA), Some(data));
                receiver.step_instruction().unwrap();
                assert_eq!(
                    receiver.registers().d[0],
                    u32::from(if boot_mapped { opcode } else { 7 })
                );
                for _ in 0..3 {
                    receiver.step_instruction().unwrap();
                }
                assert_eq!(receiver.registers().d[0], u32::from(opcode));
                for identity in 0..2 {
                    let (mut rom, mut boot) = images(&program);
                    if identity == 0 {
                        rom[DATA as usize] ^= 1;
                    } else {
                        boot[0x7fe] ^= 1;
                    }
                    let mut wrong = Emulator::new_with_tmss(
                        rom,
                        LoadConfig {
                            region,
                            concurrent_sound: concurrent,
                        },
                        boot,
                    )
                    .unwrap();
                    wrong.install_rom_patches(&patches(set)).unwrap();
                    enter_cartridge(&mut wrong);
                    wrong.step_instruction().unwrap();
                    failure_preserves(&mut wrong, &saved[codec], codec);
                }
            }
        }
    }
}
