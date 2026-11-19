pub(crate) fn dram_storage_contract() {
    pixel_modes();
    readback_agreement();
    readback_transaction();
    memory_mode_levels();
    ordinary_strides();
    use crate::svp::controller::Controller;
    use crate::svp::{InstructionModel, Progress};
    use zeff_emu_common::save_state::{StateReader, StateWriter};

    fn controller_bytes(model: &InstructionModel) -> Vec<u8> {
        let mut writer = StateWriter::new();
        model.ports.write_state(&mut writer);
        writer.into_bytes()
    }

    let mut irom = vec![0; 1024];
    irom[0x3fc] = 0xfc00;
    let mut original = InstructionModel::new_synthetic(vec![0], irom).unwrap();
    let reset = original.encode_state().unwrap();
    original.decode_state(&reset).unwrap();
    assert_eq!(original.encode_state().unwrap(), reset);
    assert_eq!(
        original.advance(true).unwrap(),
        Progress::ResetVector { entry: 0xfc00 }
    );

    let initial = [
        (0x00ff, 0x1111),
        (0x0100, 0x2222),
        (0xff00, 0x4444),
        (0xffff, 0x3333),
    ];
    for (address, value) in initial {
        original.main_dram_write(address, value, 0xffff).unwrap();
    }
    let mut detached = original.clone();
    original.main_dram_write(0x00ff, 0x00aa, 0x00ff).unwrap();
    detached.main_dram_write(0x0100, 0xab00, 0xff00).unwrap();
    detached.main_dram_write(0xff00, 0x0055, 0x00ff).unwrap();
    detached.main_dram_write(0xffff, 0xbeef, 0xffff).unwrap();
    assert_eq!(
        initial.map(|(address, _)| original.peek_dram(address)),
        [0x11aa, 0x2222, 0x4444, 0x3333]
    );
    assert_eq!(
        initial.map(|(address, _)| detached.peek_dram(address)),
        [0x1111, 0xab22, 0x4455, 0xbeef]
    );
    for address in [0x00fe, 0x0101, 0xfeff, 0xff01, 0xfffe] {
        assert_eq!(
            (original.peek_dram(address), detached.peek_dram(address)),
            (0, 0)
        );
    }

    let wire = controller_bytes(&detached);
    let dram_offset = 20 + 1024 * 2;
    let mut expected = vec![0; dram_offset + 65536 * 2 + 9];
    expected[dram_offset + 65536 * 2..dram_offset + 65536 * 2 + 4].copy_from_slice(b"PMCS");
    for (address, value) in [
        (0x00ff, 0x1111u16),
        (0x0100, 0xab22),
        (0xff00, 0x4455),
        (0xffff, 0xbeef),
    ] {
        let offset = dram_offset + address * 2;
        expected[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
    }
    assert!(wire == expected, "DRAM state wire differs");
    let mut reader = StateReader::new(&wire);
    let restored = Controller::read_state(&mut reader, detached.rom.clone()).unwrap();
    assert!(reader.is_exhausted());
    assert!(restored == detached.ports, "restored controller differs");

    let snapshot = detached.encode_state().unwrap();
    let truncated = &snapshot[..snapshot.len() - 2];
    assert!(detached.decode_state(truncated).is_err());
    assert_eq!(detached.encode_state().unwrap(), snapshot);
    let mut noncanonical = wire;
    noncanonical[17] = 1;
    let mut reader = StateReader::new(&noncanonical);
    assert!(Controller::read_state(&mut reader, detached.rom.clone()).is_err());
    assert_eq!(detached.encode_state().unwrap(), snapshot);
}

fn pixel_modes() {
    use crate::svp::controller::Controller;
    use crate::svp::ssp1601::Ports;
    use crate::svp::{InstructionModel, Progress};

    fn program(ports: &mut Controller, address: u16, mode: u16) {
        ports.write(6, 0x60, false, address).unwrap();
        ports.write(6, 0x60, false, mode).unwrap();
        ports.write(4, 0x60, true, 0xffff).unwrap();
    }

    for (incoming, expected) in [
        (0x0000, 0x1234),
        (0x000a, 0x123a),
        (0x00a0, 0x12a4),
        (0x00aa, 0x12aa),
        (0x0a00, 0x1a34),
        (0x0a0a, 0x1a3a),
        (0x0aa0, 0x1aa4),
        (0x0aaa, 0x1aaa),
        (0xa000, 0xa234),
        (0xa00a, 0xa23a),
        (0xa0a0, 0xa2a4),
        (0xa0aa, 0xa2aa),
        (0xaa00, 0xaa34),
        (0xaa0a, 0xaa3a),
        (0xaaa0, 0xaaa4),
        (0xaaaa, 0xaaaa),
    ] {
        let mut ports = Controller::synthetic();
        ports.main_dram_write(0x100, 0x1234, 0xffff);
        program(&mut ports, 0x100, 0x0418);
        let before = ports.clone();
        ports.write(4, 0x60, false, incoming).unwrap();
        assert_eq!((ports.dram(0x100), before.dram(0x100)), (expected, 0x1234));
        assert_eq!(ports.read(6, 0x60, false).unwrap(), 0x100);
    }

    for mode in [0x4018, 0x4418] {
        for (start, addresses) in [
            (0x100, [0x100, 0x101, 0x120, 0x121]),
            (0x101, [0x101, 0x120, 0x121, 0x140]),
            (0xfe, [0xfe, 0xff, 0x11e, 0x11f]),
        ] {
            let mut ports = Controller::synthetic();
            for address in addresses {
                ports.main_dram_write(address, 0x1234, 0xffff);
            }
            program(&mut ports, start, mode);
            for address in addresses {
                ports.write(4, 0x60, false, 0xa000).unwrap();
                assert_eq!(
                    ports.dram(address),
                    if mode == 0x4418 { 0xa234 } else { 0xa000 }
                );
            }
            let mut model = super::fresh();
            model.advance(true).unwrap();
            model.ports = ports;
            let saved = model.encode_state().unwrap();
            model.decode_state(&saved).unwrap();
            assert_eq!(model.encode_state().unwrap(), saved);
        }
    }

    let mut ports = Controller::synthetic();
    ports.main_dram_write(0x100, 0x1234, 0xffff);
    program(&mut ports, 0x100, 0x0c18);
    ports.write(4, 0x60, false, 0).unwrap();
    ports.write(4, 0x60, false, 0xabcd).unwrap();
    assert_eq!((ports.dram(0x100), ports.dram(0x101)), (0x1234, 0xabcd));
    ports.main_dram_write(0x104, 0x5678, 0xffff);
    ports.write(6, 0x60, false, 0x104).unwrap();
    ports.read(6, 0x60, true).unwrap();
    ports.write(4, 0x60, true, 0xffff).unwrap();
    ports.write(4, 0x60, false, 0xb000).unwrap();
    assert_eq!(ports.dram(0x104), 0xb678);
    program(&mut ports, 0x100, 0x9418);
    ports.main_dram_write(0xfe, 0x5678, 0xffff);
    ports.write(4, 0x60, false, 0).unwrap();
    ports.write(4, 0x60, false, 0xa000).unwrap();
    assert_eq!((ports.dram(0x100), ports.dram(0xfe)), (0x1234, 0xa678));

    let mut edge = Controller::synthetic();
    edge.main_dram_write(0xfffe, 0x1234, 0xffff);
    edge.main_dram_write(0xffff, 0x5678, 0xffff);
    edge.main_dram_write(0x1e, 0x9abc, 0xffff);
    program(&mut edge, 0xfffe, 0x4418);
    edge.write(4, 0x60, false, 0xa000).unwrap();
    edge.write(4, 0x60, false, 0xb000).unwrap();
    let mut model = super::fresh();
    model.advance(true).unwrap();
    model.ports = edge;
    let saved = model.encode_state().unwrap();
    model.decode_state(&saved).unwrap();
    assert_eq!(model.encode_state().unwrap(), saved);
    assert!(model.ports.write(4, 0x60, false, 0xffff).is_err());
    assert_eq!(model.encode_state().unwrap(), saved);
    assert_eq!(
        (
            model.peek_dram(0xfffe),
            model.peek_dram(0xffff),
            model.peek_dram(0x1e)
        ),
        (0xa234, 0xb678, 0x9abc)
    );
    assert_eq!(model.ports.read(6, 0x60, false).unwrap(), 0x1e);

    let mut encoded = zeff_emu_common::save_state::StateWriter::new();
    model.ports.write_state(&mut encoded);
    let mut malformed = encoded.into_bytes();
    // One partial-address word precedes the sole write channel in this fixture.
    malformed[18..20].copy_from_slice(&0xc400_u16.to_le_bytes());
    let mut reader = zeff_emu_common::save_state::StateReader::new(&malformed);
    assert!(Controller::read_state(&mut reader, model.rom.clone()).is_err());
    for mode in [0xc018, 0x4818, 0x401c, 0x0400, 0x4618] {
        let mut ports = Controller::synthetic();
        ports.write(6, 0, false, 0).unwrap();
        let before = ports.clone();
        assert!(ports.write(6, 0, false, mode).is_err());
        assert_eq!(ports, before);
    }
    let mut ports = Controller::synthetic();
    ports.write(6, 0, false, 0).unwrap();
    ports.write(6, 0, false, 0x4418).unwrap();
    let before = ports.clone();
    assert!(ports.read(4, 0, true).is_err());
    assert_eq!(ports, before);

    let code = [
        0x0840, 0x60, 0x08e0, 0x100, 0x08e0, 0x4418, 0x00c0, 0x0810, 0xa000, 0x00c1, 0x00c1,
    ];
    let mut rom = vec![0; 0x420];
    rom[0x400..0x400 + code.len()].copy_from_slice(&code);
    let mut irom = vec![0; 1024];
    irom[0x3fc] = 0xfc00;
    irom[0..2].copy_from_slice(&[0x0860, 0x400]);
    let template = InstructionModel::new_synthetic(rom, irom).unwrap();
    let mut model = template.clone();
    model.advance(true).unwrap();
    model.main_dram_write(0x100, 0x1234, 0xffff).unwrap();
    model.main_dram_write(0x101, 0x5678, 0xffff).unwrap();
    let mut done = false;
    for _ in 0..40 {
        let saved = model.encode_state().unwrap();
        let mut restored = template.clone();
        restored.decode_state(&saved).unwrap();
        let expected = model.advance(true).unwrap();
        assert_eq!(restored.advance(true).unwrap(), expected);
        assert_eq!(
            restored.encode_state().unwrap(),
            model.encode_state().unwrap()
        );
        if matches!(expected, Progress::Prepared { pc: 0x409, .. }) {
            assert_eq!(model.peek_dram(0x100), 0x1234);
        }
        if matches!(expected, Progress::Committed { pc: 0x40a, .. }) {
            done = true;
            break;
        }
    }
    assert!(done);
    assert_eq!(
        (model.peek_dram(0x100), model.peek_dram(0x101)),
        (0xa234, 0xa678)
    );
}

fn ordinary_strides() {
    use crate::svp::controller::Controller;
    use crate::svp::ssp1601::Ports;
    for (code, magnitude) in [0_i32, 1, 2, 4, 8, 16, 32, 128].into_iter().enumerate() {
        for negative in [false, true] {
            let mode = 0x18 | (code as u16) << 11 | if negative { 0x8000 } else { 0 };
            let step = if negative { -magnitude } else { magnitude };
            let mut ports = Controller::synthetic();
            ports.write(6, 0, false, 0x100).unwrap();
            ports.write(6, 0, false, mode).unwrap();
            ports.write(4, 0, true, 0xffff).unwrap();
            ports.write(4, 0, false, 0x55aa).unwrap();
            ports.write(4, 0, false, 0x1234).unwrap();
            assert_eq!(ports.dram((0x100 + step) as u16), 0x1234);
            if magnitude != 0 {
                assert_eq!(ports.dram(0x100), 0x55aa);
            }
            ports.read(6, 0, true).unwrap();
            ports.read(6, 0, true).unwrap();
            ports.write(4, 0, true, 0xffff).unwrap();
            ports.write(4, 0, false, 0xabcd).unwrap();
            assert_eq!(ports.dram((0x100 + 2 * step) as u16), 0xabcd);
            let mut model = super::fresh();
            model.advance(true).unwrap();
            model.ports = ports;
            let snapshot = model.encode_state().unwrap();
            model.decode_state(&snapshot).unwrap();
            assert!(model.encode_state().unwrap() == snapshot);
        }
    }
}

fn memory_mode_levels() {
    use crate::svp::controller::Controller;
    use crate::svp::ssp1601::Ports;
    for st in [0x20, 0x60] {
        let mut ports = Controller::synthetic();
        ports.write(6, st, false, 0xff).unwrap();
        ports.write(6, st, false, 0x0818).unwrap();
        ports.write(4, st, true, 0xffff).unwrap();
        ports.write(4, st, false, 0x55aa).unwrap();
        ports.write(4, st, false, 0x1234).unwrap();
        assert_eq!((ports.dram(0xff), ports.dram(0x100)), (0x55aa, 0x1234));
        ports.write(6, st, false, 0x200).unwrap();
        ports.read(6, st, true).unwrap();
        ports.write(4, st, true, 0xffff).unwrap();
        ports.write(4, st, false, 0xabcd).unwrap();
        assert_eq!(ports.dram(0x200), 0xabcd);
        ports.read(6, st, true).unwrap();
        ports.read(6, st, true).unwrap();
        ports.write(4, st, true, 0xffff).unwrap();
        ports.write(4, st, false, 0x7788).unwrap();
        assert_eq!(ports.dram(0x201), 0x7788);
        ports.write(6, st, false, 0x300).unwrap();
        ports.write(6, st, false, 0x0018).unwrap();
        ports.write(6, st, false, 0x400).unwrap();
        ports.write(6, st, false, 0x0818).unwrap();
        assert_eq!(ports.read(6, st, false).unwrap(), 0x400);
        ports.read(6, st, true).unwrap();
        ports.write(4, st, true, 0xffff).unwrap();
        ports.write(4, st, false, 0xcdef).unwrap();
        assert_eq!((ports.dram(0x300), ports.dram(0x400)), (0, 0xcdef));
        let before = ports.clone();
        assert!(ports.write(4, 0x40, false, 0xbeef).is_err());
        assert!(ports.read(4, 0x40, false).is_err());
        assert!(ports == before);
        let mut model = super::fresh();
        model.advance(true).unwrap();
        model.ports = ports;
        let snapshot = model.encode_state().unwrap();
        let mut forged = snapshot.clone();
        let start = forged.len() - 6;
        let end = forged.len() - 1;
        forged[start..end].fill(0);
        assert!(model.decode_state(&forged).is_err());
        assert!(model.encode_state().unwrap() == snapshot);
    }
}

fn readback_agreement() {
    use crate::svp::controller::Controller;
    use crate::svp::ssp1601::Ports;
    for (input, expected) in [
        (0x0000, 0x0000),
        (0x1111, 0x1111),
        (0xffff, 0xffff),
        (0x4545, 0x5454),
        (0xa1a2, 0x1a2a),
        (0x0f01, 0xf010),
    ] {
        let mut ports = Controller::synthetic();
        ports.write(6, 0, false, input).unwrap();
        ports.write(6, 0, false, 0x4418).unwrap();
        ports.write(4, 0, true, 0xffff).unwrap();
        ports.write(6, 0, false, input).unwrap();
        assert_eq!(ports.read(6, 0, false).unwrap(), expected);
        ports.write(4, 0, true, 0xffff).unwrap();
        ports.write(4, 0, false, 0xabcd).unwrap();
        assert_eq!(ports.dram(input), 0xabcd);
        let mut model = super::fresh();
        model.advance(true).unwrap();
        model.ports = ports;
        let saved = model.encode_state().unwrap();
        model.decode_state(&saved).unwrap();
        assert_eq!(model.encode_state().unwrap(), saved);
    }
    for input in [0x1234, 0x1000, 0x0010, 0xabcd] {
        let mut ports = Controller::synthetic();
        ports.write(6, 0, false, input).unwrap();
        ports.write(6, 0, false, 0x4418).unwrap();
        ports.write(4, 0, true, 0xffff).unwrap();
        ports.write(6, 0, false, input).unwrap();
        let before = ports.clone();
        assert!(ports.read(6, 0, false).is_err());
        assert_eq!(ports, before);
        let mut cpu = super::Core::synthetic(0);
        let before_cpu = cpu.clone();
        let mut candidate = ports.clone();
        assert!(cpu.step(|_| Ok(0x04e3), &mut candidate).is_err());
        assert_eq!(cpu, before_cpu);
        assert_eq!(candidate, before);
    }
}

fn readback_transaction() {
    use crate::svp::{InstructionModel, Progress};
    let code = [
        0x0840, 0x60, 0x08e0, 0x4545, 0x08e0, 0x4418, 0x00c0, 0x08e0, 0x4545, 0x04e3, 0x000f,
    ];
    let mut rom = vec![0; 0x420];
    rom[0x400..0x400 + code.len()].copy_from_slice(&code);
    let mut irom = vec![0; 1024];
    irom[0x3fc] = 0xfc00;
    irom[0..2].copy_from_slice(&[0x0860, 0x400]);
    let template = InstructionModel::new_synthetic(rom, irom).unwrap();
    let mut model = template.clone();
    let mut prepared = false;
    let mut done = false;
    for _ in 0..40 {
        let saved = model.encode_state().unwrap();
        let mut restored = template.clone();
        restored.decode_state(&saved).unwrap();
        let progress = model.advance(true).unwrap();
        assert_eq!(restored.advance(true).unwrap(), progress);
        assert_eq!(
            restored.encode_state().unwrap(),
            model.encode_state().unwrap()
        );
        if matches!(progress, Progress::Prepared { pc: 0x409, .. }) {
            prepared = true;
            assert_eq!(model.core.ram[0][0], 0);
            let before = model.encode_state().unwrap();
            assert_eq!(
                model.publish_main_command(0x1234),
                Err(crate::svp::Error::Busy)
            );
            assert_eq!(model.encode_state().unwrap(), before);
        }
        if matches!(progress, Progress::Committed { pc: 0x409, .. }) {
            assert_eq!(model.core.ram[0][0], 0x5454);
        }
        if matches!(progress, Progress::Committed { pc: 0x40a, .. }) {
            done = true;
            break;
        }
    }
    assert!(prepared && done);
    assert_eq!(model.core.ram[0][0], 0x5454);
}
