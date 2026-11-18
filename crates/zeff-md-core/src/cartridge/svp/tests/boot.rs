use super::*;
use std::sync::Arc;

fn program(ports: &mut Controller, index: u8, address: u16, mode: u16, writing: bool) {
    ports.write(6, 0, false, address).unwrap();
    ports.write(6, 0, false, mode).unwrap();
    if writing {
        ports.write(index, 0, true, 0xffff).unwrap();
    } else {
        ports.read(index, 0, true).unwrap();
    }
}

pub(super) fn controller_boot_contract() {
    authored_boot();
    let mut cpu = Core::synthetic(0);
    let mut ports = Controller::with_rom(Arc::from([0x5356, 0x1401, 0, 0x456]));
    let initial = ports.clone();
    cpu_word(&mut cpu, &mut ports, &[0x000d]);
    assert_ne!(ports, initial);
    reject_cpu_word(&cpu, &ports, &[0x000d]);
    for words in [&[0x08e0, 0x55][..], &[0x000f], &[0x000f]] {
        cpu_word(&mut cpu, &mut ports, words);
    }
    program(&mut ports, 4, 0x83fa, 0x081c, true);
    for (offset, word) in [0x860, 0xfc04, 0x860, 0xfc04, 0x860, 0xfc04]
        .into_iter()
        .enumerate()
    {
        ports.write(4, 0, false, word).unwrap();
        assert_eq!(ports.iram[0x3fa + offset], word);
    }
    let at_end = ports.clone();
    assert!(ports.write(4, 0, false, 0xdead).is_err());
    assert_eq!(ports, at_end);
    program(&mut ports, 4, 0, 0x0800, false);
    assert_eq!(ports.read(4, 0, false).unwrap(), 0x5356);
    assert_eq!(ports.read(4, 0, false).unwrap(), 0x1401);
    assert_eq!(
        &ports.iram[0x3fa..],
        &[0x860, 0xfc04, 0x860, 0xfc04, 0x860, 0xfc04]
    );
    ports.write(0, 0, false, 0xf000).unwrap();
    ports.publish_main(0x1234);
    assert_eq!(ports.read(0, 0, false).unwrap(), 0xf002);
    assert_eq!(ports.status(), 0xf000);
    ports.write(3, 0, false, 0xabcd).unwrap();
    assert_eq!(ports.consume_main_status(), 0xf001);
    assert_eq!((ports.status(), ports.xst()), (0xf000, 0xabcd));
    let mut w = StateWriter::new();
    ports.write_state(&mut w);
    let bytes = w.into_bytes();
    let mut r = zeff_emu_common::save_state::StateReader::new(&bytes);
    assert_eq!(
        Controller::read_state(&mut r, Arc::from([0x5356, 0x1401, 0, 0x456])).unwrap(),
        ports
    );
    assert_eq!(ports.read(4, 0, false).unwrap(), 0);
    assert_eq!(ports.read(4, 0, false).unwrap(), 0x456);
    assert!(ports.read(4, 0, false).is_err());
    let mut malformed = bytes.clone();
    let cold = malformed.len() - (1024 + 65536) * 2 - 3 - 9;
    malformed[cold] = 1;
    let mut r = zeff_emu_common::save_state::StateReader::new(&malformed);
    assert!(Controller::read_state(&mut r, Arc::from([0x5356, 0x1401, 0, 0x456])).is_err());
    for st in [0, 0x20, 0x40] {
        let before = ports.clone();
        assert!(ports.read(5, st, true).is_err());
        assert_eq!(ports, before);
        assert!(ports.read(5, st, false).is_err());
        assert_eq!(ports, before);
    }
}

fn authored_boot() {
    for valid in [false, true] {
        let mut rom = vec![0; 0x600];
        rom[0x100] = if valid { 0x5a5a } else { 0xaaaa };
        rom[0x101] = 0x456;
        let mut irom = vec![0; 1024];
        irom[0x3fc] = 0xfc08;
        irom[0..2].copy_from_slice(&[0x0860, 0xfc00]);
        let code = [
            0x0840, 0, 0x000d, 0x08e0, 0x1234, 0x000f, 0x000f, 0x08e0, 0x0100, 0x08e0, 0x0800,
            0x000c, 0x003c, 0x6800, 0x5a5a, 0x4c50, 0xfc00, 0x006c,
        ];
        irom[8..8 + code.len()].copy_from_slice(&code);
        let mut model = InstructionModel::new_synthetic(rom.clone(), irom.clone()).unwrap();
        for _ in 0..100 {
            let bytes = model.encode_state().unwrap();
            let mut fresh = InstructionModel::new_synthetic(rom.clone(), irom.clone()).unwrap();
            fresh.decode_state(&bytes).unwrap();
            let step = model.advance(true).unwrap();
            assert_eq!(fresh.advance(true).unwrap(), step);
            assert_eq!(fresh, model);
            if matches!(step, Progress::Committed { .. })
                && model.registers().pc == if valid { 0x456 } else { 0xfc00 }
            {
                break;
            }
        }
        assert_eq!(model.registers().pc, if valid { 0x456 } else { 0xfc00 });
    }
}
