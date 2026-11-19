use super::*;

pub(super) fn codec_profiles() {
    let mut cpu = Cpu::new_with_memptr_profile(ResetState::new(0, 0x8000), MemptrProfile::Nmos);
    let mut memory = Bus::new(&[0x3a, 0xff, 0x27, 0x21, 0, 0x40, 0xcb, 0x7e]);
    let mut bus = BusState::default();
    cpu.step_with_bus(&mut memory).unwrap();
    cpu.step_with_bus(&mut memory).unwrap();
    cpu.step_bus_segment(&mut memory, &mut bus).unwrap();
    cpu.step_bus_segment(&mut memory, &mut bus).unwrap();
    assert_eq!(cpu.memptr(), Some(0x2800));
    witness(&cpu, &bus, &memory);
    let bytes = encode(&cpu, &bus);
    for offset in [bytes.len() - 4, bytes.len() - 3] {
        let mut malformed = bytes.clone();
        malformed[offset] = 2;
        let before = (cpu.clone(), bus);
        assert!(
            cpu.read_continuation(&mut bus, &mut Reader::new(&malformed))
                .is_err()
        );
        assert_eq!((cpu.clone(), bus), before);
    }
    let before = (cpu.clone(), bus);
    let mut legacy = Writer::new();
    Cpu::new()
        .write_legacy_continuation(&BusState::default(), &mut legacy)
        .unwrap();
    let legacy = legacy.into_bytes();
    assert!(
        cpu.read_continuation(&mut bus, &mut Reader::new(&legacy))
            .is_err()
    );
    assert!(
        cpu.read_legacy_continuation(&mut bus, &mut Reader::new(&legacy))
            .is_err()
    );
    assert!(
        cpu.write_legacy_continuation(&bus, &mut Writer::new())
            .is_err()
    );
    assert_eq!((cpu.clone(), bus), before);

    let mut profile = bytes.clone();
    profile[bytes.len() - 4] = 0;
    assert!(
        cpu.read_continuation(&mut bus, &mut Reader::new(&profile))
            .is_err()
    );
    cpu.reset();
    assert_eq!(cpu.memptr(), None);
    assert_eq!(cpu.memptr_profile(), MemptrProfile::Nmos);
    witness(&cpu, &BusState::default(), &memory);
    let mut unknown = encode(&cpu, &BusState::default());
    *unknown.last_mut().unwrap() = 1;
    assert!(
        cpu.read_continuation(&mut bus, &mut Reader::new(&unknown))
            .is_err()
    );
    producer_values();
}

fn producer_values() {
    for (program, expected) in [
        (&[0x3e, 0x28, 0x01, 0xff, 0x12, 0x02][..], Some(0x2800)),
        (&[0x3e, 0x28, 0x32, 0xff, 0x12][..], Some(0x2800)),
        (&[0x21, 0xff, 0x27, 0x09][..], Some(0x2800)),
        (&[0xdd, 0x21, 0xff, 0x27, 0xdd, 0x46, 1][..], Some(0x2800)),
        (&[0x01, 0xff, 0x27, 0xed, 0x40][..], Some(0x2800)),
        (&[0x01, 0xff, 0x27, 0xed, 0x48][..], Some(0x2800)),
        (&[0x3a, 0xff, 0x27, 0xed, 0xa0][..], Some(0x2800)),
        (&[0xed, 0xa1][..], None),
        (&[0x3a, 0xff, 0x27, 0xed, 0xa9][..], Some(0x27ff)),
    ] {
        let mut cpu = Cpu::new_with_memptr_profile(ResetState::new(0, 0x8000), MemptrProfile::Nmos);
        let mut memory = Bus::new(program);
        while usize::from(cpu.regs.pc) < program.len() {
            cpu.step_with_bus(&mut memory).unwrap();
            assert!(cpu.trap().is_none());
        }
        assert_eq!(cpu.memptr(), expected, "{program:02x?}");
    }
}
