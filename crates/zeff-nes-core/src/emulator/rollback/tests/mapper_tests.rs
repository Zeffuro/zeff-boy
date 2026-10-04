use super::*;

fn banked_rom(mapper: u8) -> Vec<u8> {
    let mut rom = vec![0; 16 + 0x8000 + if mapper == 4 { 0x2000 } else { 0 }];
    rom[..4].copy_from_slice(b"NES\x1a");
    rom[4] = 2;
    rom[5] = u8::from(mapper == 4);
    rom[6] = mapper << 4;
    let bank_size = if mapper == 2 { 0x4000 } else { 0x2000 };
    for bank in 0..0x8000 / bank_size {
        rom[16 + bank * bank_size] = 0x31 + bank as u8;
    }
    let mut reset = vec![
        0x78, 0xd8, 0xa2, 0xff, 0x9a, 0xa9, 0x00, 0x85, 0x00, 0x85, 0x02, 0x85, 0x03, 0x8d, 0x00,
        0x20, 0x8d, 0x01, 0x20, 0xa9, 0x40, 0x8d, 0x17, 0x40, 0xa9, 0x01, 0x8d, 0x15, 0x40, 0xa9,
        0xbf, 0x8d, 0x00, 0x40, 0xa9, 0x60, 0x8d, 0x02, 0x40, 0xa9, 0x08, 0x8d, 0x03, 0x40,
    ];
    if mapper == 4 {
        reset.extend_from_slice(&[
            0xa9, 0x02, 0x8d, 0x00, 0xc0, 0x8d, 0x01, 0xc0, 0x8d, 0x01, 0xe0,
        ]);
    }
    reset.extend_from_slice(&[
        0xa9, 0x90, 0x8d, 0x00, 0x20, 0xa9, 0x1e, 0x8d, 0x01, 0x20, 0x58,
    ]);
    let idle = 0xe000u16 + reset.len() as u16;
    reset.extend_from_slice(&[0x4c, idle as u8, (idle >> 8) as u8]);
    let mut nmi = vec![
        0x48, 0x8a, 0x48, 0xa9, 0x01, 0x8d, 0x16, 0x40, 0xa9, 0x00, 0x8d, 0x16, 0x40, 0x85, 0x00,
        0xa2, 0x08, 0xad, 0x16, 0x40, 0x4a, 0x26, 0x00, 0xca, 0xd0, 0xf7,
    ];
    if mapper == 4 {
        nmi.extend_from_slice(&[0xa9, 0x06, 0x8d, 0x00, 0x80]);
    }
    nmi.extend_from_slice(&[
        0xa5,
        0x00,
        0x29,
        0x01,
        0x8d,
        if mapper == 4 { 0x01 } else { 0x00 },
        0x80,
        0xad,
        0x00,
        0x80,
        0x85,
        0x02,
        0x8d,
        0x02,
        0x40,
        0x68,
        0xaa,
        0x68,
        0x40,
    ]);
    let irq = [
        0x48, 0xe6, 0x03, 0xa9, 0x00, 0x8d, 0x00, 0xe0, 0x8d, 0x01, 0xe0, 0x68, 0x40,
    ];
    let fixed = &mut rom[16 + 0x6000..16 + 0x8000];
    fixed[..reset.len()].copy_from_slice(&reset);
    fixed[0x100..0x100 + nmi.len()].copy_from_slice(&nmi);
    fixed[0x200..0x200 + irq.len()].copy_from_slice(&irq);
    fixed[0x1ffa..].copy_from_slice(&[0x00, 0xe1, 0x00, 0xe0, 0x00, 0xe2]);
    rom
}

#[test]
fn corrected_banked_trajectories_restore_mapping_and_mmc3_irqs() {
    for mapper in [2, 4] {
        let rom = banked_rom(mapper);
        for phase in [3, 4] {
            for depth in [1, 8] {
                let mut subject = Emulator::new(&rom, 48_000.0).unwrap();
                let mut control = Emulator::new(&rom, 48_000.0).unwrap();
                let session = subject.begin_rollback_session().unwrap();
                for _ in 0..phase {
                    let expected = ordinary_frame(&mut control, [0, 0]);
                    let actual = session.advance_frame(&mut subject, [0, 0]).unwrap();
                    assert_eq!(observe(&subject, actual), observe(&control, expected));
                }
                assert_eq!(subject.cpu_peek(2), 0x31);
                let snapshot = session.capture(&subject).unwrap();
                let before = observe(&subject, Vec::new());
                for _ in 0..depth {
                    session.advance_frame(&mut subject, [0x80, 0]).unwrap();
                }
                assert_eq!(subject.cpu_peek(2), 0x32);
                assert_eq!(subject.cpu_peek(0x8000), 0x32);
                if mapper == 4 {
                    assert!(
                        subject.cpu_irq_count() > 0,
                        "fixture must execute MMC3 IRQs"
                    );
                }
                let inputs: Vec<_> = (0..depth + 3)
                    .map(|frame| [if frame % 2 == 0 { 0 } else { 0x80 }, 0])
                    .collect();
                let expected: Vec<_> = inputs
                    .iter()
                    .map(|ports| {
                        let audio = ordinary_frame(&mut control, *ports);
                        observe(&control, audio)
                    })
                    .collect();
                for _ in 0..2 {
                    session.restore(&mut subject, &snapshot).unwrap();
                    assert_eq!(observe(&subject, Vec::new()), before);
                    assert_eq!(subject.cpu_peek(0x8000), 0x31);
                    for (ports, reference) in inputs.iter().zip(&expected) {
                        let audio = session.advance_frame(&mut subject, *ports).unwrap();
                        assert_eq!(&observe(&subject, audio), reference);
                    }
                }
                assert!(
                    expected
                        .iter()
                        .any(|frame| frame.audio.iter().any(|bits| *bits != 0))
                );
                if mapper == 4 {
                    assert!(control.cpu_irq_count() > 0);
                }
            }
        }
    }
}

#[test]
fn mmc3_rollback_preserves_a12_filter_reload_and_pending_irq() {
    let mut subject = Emulator::new(&banked_rom(4), 48_000.0).unwrap();
    let session = subject.begin_rollback_session().unwrap();
    subject.bus.cartridge.cpu_write(0xc000, 0);
    subject.bus.cartridge.cpu_write(0xc001, 0);
    subject.bus.cartridge.cpu_write(0xe001, 0);
    subject.bus.cartridge.notify_ppu_a12(true, 8);
    assert!(subject.bus.cartridge.irq_pending());
    let pending = session.capture(&subject).unwrap();
    subject.bus.cartridge.cpu_write(0xe000, 0);
    subject.bus.cartridge.cpu_write(0xe001, 0);
    subject.bus.cartridge.notify_ppu_a12(false, 10);
    let low = session.capture(&subject).unwrap();
    subject.bus.cartridge.notify_ppu_a12(true, 18);
    assert!(subject.bus.cartridge.irq_pending());
    session.restore(&mut subject, &low).unwrap();
    assert!(!subject.bus.cartridge.irq_pending());
    subject.bus.cartridge.notify_ppu_a12(true, 17);
    assert!(!subject.bus.cartridge.irq_pending());
    subject.bus.cartridge.notify_ppu_a12(false, 18);
    subject.bus.cartridge.notify_ppu_a12(true, 26);
    assert!(subject.bus.cartridge.irq_pending());
    session.restore(&mut subject, &pending).unwrap();
    assert!(subject.bus.cartridge.irq_pending());
}
