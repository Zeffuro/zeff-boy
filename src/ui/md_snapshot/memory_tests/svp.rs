use super::*;
use zeff_md_core::svp::FunctionalProfile;

fn images() -> (Vec<u8>, Vec<u16>) {
    let mut rom = vec![0; 0x1000];
    rom[..8].copy_from_slice(&[0, 0xff, 1, 0, 0, 0, 1, 0]);
    let mut main = vec![
        0x33fc, 0x1234, 0x00a1, 0x5000, 0x33fc, 0x7788, 0x00ff, 0x0200, 0x1039, 0x00a1, 0x5005,
        0x0200, 1, 0x67f4, 0x3239, 0x00a1, 0x5000, 0x3439, 0x0030, 0, 0x3639, 0x0030, 2,
    ];
    main.extend_from_slice(&[
        0x13fc, 0xab, 0x30, 4, 0x13fc, 0xcd, 0x30, 5, 0x3839, 0x30, 4,
    ]);
    for value in [
        0x8114, 0x8f02, 0x9302, 0x9400, 0x9500, 0x9600, 0x9718, 0x4400, 0x0080,
    ] {
        main.extend_from_slice(&[0x33fc, value, 0x00c0, 4]);
    }
    main.extend_from_slice(&[0x4e71, 0x60fe]);
    for (i, word) in main.into_iter().enumerate() {
        rom[0x100 + i * 2..0x102 + i * 2].copy_from_slice(&u16::to_be_bytes(word));
    }
    let dsp = [
        0x0840, 0, 0x0038, 0xa800, 2, 0x4d50, 0x402, 0x003b, 0x8800, 1, 0x0013, 0x0840, 0x20,
        0x08e0, 0, 0x08e0, 0x0818, 0x0080, 0x0081, 0x0081, 0x0840, 0, 0x00b1, 0x4c00, 0x416,
    ];
    for (i, word) in dsp.into_iter().enumerate() {
        rom[0x800 + i * 2..0x802 + i * 2].copy_from_slice(&u16::to_be_bytes(word));
    }
    let mut irom = vec![0; 1024];
    irom[8..10].copy_from_slice(&[0x0860, 0x400]);
    irom[0x3fc] = 0xfc08;
    (rom, irom)
}

fn functional() -> Emulator {
    let (rom, irom) = images();
    Emulator::new_functional_svp(
        rom,
        LoadConfig {
            region: Region::NtscUs,
            concurrent_sound: false,
        },
        irom,
        FunctionalProfile::new(0x418, 1, 3, [1, 1, 2, 2, 1, 1]).unwrap(),
    )
    .unwrap()
}

fn states(emu: &Emulator) -> (Vec<u8>, Vec<u8>, Vec<u8>) {
    (
        emu.encode_state().unwrap(),
        emu.encode_frontend_state().unwrap(),
        emu.encode_debugger_checkpoint().unwrap(),
    )
}

pub(super) fn contract() {
    let mut subject = functional();
    let mut control = functional();
    for _ in 0..256 {
        assert_eq!(
            subject.step_instruction().unwrap(),
            control.step_instruction().unwrap()
        );
        if subject.registers().d[4] == 0xabcd {
            break;
        }
    }
    assert_eq!(subject.registers().d[4], 0xabcd);
    assert!(!subject.supports_rom_cheats());
    assert!(
        subject
            .synthetic_svp()
            .unwrap()
            .registers()
            .committed_instructions
            > 0
    );
    assert_eq!(
        (0xa15000..=0xa15005)
            .map(|address| subject.peek(address))
            .collect::<Vec<_>>(),
        [
            Some(0x12),
            Some(0x35),
            Some(0x12),
            Some(0x35),
            Some(0),
            Some(1)
        ]
    );
    for address in [0xa15006, 0xa15007, 0xa15008, 0xa150ff, 0xa15100] {
        assert_eq!(subject.peek(address), None);
    }
    let before = states(&subject);
    assert_eq!(before, states(&control));
    assert_eq!(&before.0[..12], b"ZMDRAW01\x01\0\0\0");
    assert_eq!(&before.1[..12], b"ZMDFRONT\x01\0\0\0");
    let mut req = request();
    req.show_memory_viewer = false;
    for pattern in [vec![0x12, 0x35, 0x12, 0x35, 0, 1], vec![0xff]] {
        req.memory_search = Some(MemorySearchRequest {
            pattern: pattern.clone(),
            max_results: 8,
        });
        let results = collect_md_snapshot(&subject, &req, None)
            .memory_search_results
            .unwrap();
        if pattern.len() == 6 {
            assert_eq!(
                results.iter().map(|row| row.address).collect::<Vec<_>>(),
                [0xa15000]
            );
        }
        for result in results {
            assert!(result.address <= 0xffffff);
            assert!(!(0xa15006..=0xa150ff).contains(&result.address));
            assert_eq!(result.matched_bytes, pattern);
            for (offset, value) in pattern.iter().enumerate() {
                assert_eq!(subject.peek(result.address + offset as u32), Some(*value));
            }
        }
        assert_eq!(states(&subject), before);
        assert_eq!(subject, control);
    }
    for _ in 0..32 {
        assert_eq!(
            subject.step_instruction().unwrap(),
            control.step_instruction().unwrap()
        );
        assert_eq!(states(&subject), states(&control));
    }
    assert_eq!(subject.framebuffer(), control.framebuffer());
    assert!(
        subject
            .drain_audio_samples()
            .iter()
            .map(|x| x.to_bits())
            .eq(control.drain_audio_samples().iter().map(|x| x.to_bits()))
    );
}
