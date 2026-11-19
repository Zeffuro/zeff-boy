use super::*;

pub(super) fn accumulator_provenance() {
    let mut ports = TestPorts {
        inputs: Some(0),
        ..Default::default()
    };
    let mut partial = Core::synthetic(0);
    partial.a = 0x4567;
    partial.st = FLAG_N | 3;
    partial.flags_known = NZ;
    for words in [&[0x0034][..], &[0xd860][..], &[0x0043][..]] {
        one(&mut partial, &mut ports, words);
    }
    assert_eq!((partial.a, partial.a_known), (0x80634567, 0xafffffff));
    assert_eq!((partial.st, partial.flags_known), (0x8063, NZ));
    for variant in [0, FLAG_L, FLAG_OV, FLAG_L | FLAG_OV] {
        let mut concrete = Core::synthetic(0);
        concrete.a = 0x4567;
        concrete.st = FLAG_N | 3 | variant;
        for words in [&[0x0034][..], &[0xd860][..], &[0x0043][..]] {
            one(&mut concrete, &mut ports, words);
        }
        assert_eq!(
            concrete.a,
            (u32::from(FLAG_N | 0x63 | variant) << 16) | 0x4567
        );
        assert_eq!(concrete.st, FLAG_N | 0x63 | variant);
        assert_eq!(concrete.a & partial.a_known, partial.a);
        assert_eq!(concrete.st & (!FLAGS | partial.flags_known), partial.st);
        assert_eq!(concrete.a_known, u32::MAX);
    }

    let missing = Fault::UnknownAccumulator(0x50000000);
    for words in [
        &[0x0013][..],
        &[0x0083][..],
        &[0x4a10][..],
        &[0x8800, 1][..],
        &[0x6800, 0][..],
        &[0x9006][..],
        &[0x9700][..],
    ] {
        atomic_fault(&partial, &ports, words, missing.clone());
    }
    atomic_fault(
        &partial,
        &ports,
        &[0x0014],
        Fault::UnknownFlags(FLAG_L | FLAG_OV),
    );
    atomic_fault(&partial, &ports, &[0x4d40, 7], Fault::UnknownFlags(FLAG_L));
    let mut unknown_control = partial.clone();
    unknown_control.a &= !0x00200000;
    unknown_control.a_known &= !0x00200000;
    atomic_fault(
        &unknown_control,
        &ports,
        &[0x0043],
        Fault::UnknownAccumulator(0x00200000),
    );
    let mut sampled = Core::synthetic(0);
    sampled.flags_known = NZ;
    let mut input_ports = TestPorts {
        inputs: Some(0x0400),
        ..Default::default()
    };
    one(&mut sampled, &mut input_ports, &[0x0034]);
    assert_eq!(sampled.a, 0x04000000);
    atomic_fault(&sampled, &input_ports, &[0x0043], Fault::StatusMode(0x0400));

    let mut low = partial.clone();
    one(&mut low, &mut ports, &[0x001f]);
    assert_eq!(low.x, 0x4567);
    low.a = 0;
    low.a_known = 0xffff0000;
    atomic_fault(&low, &ports, &[0x001f], Fault::UnknownAccumulator(0xffff));
    low.x = 0xbeef;
    low.p = Core::product(low.x, low.y);
    one(&mut low, &mut ports, &[0x00f1]);
    assert_eq!((low.a, low.a_known), (0xbeef, u32::MAX));
    let mut upper = partial.clone();
    one(&mut upper, &mut ports, &[0x0830, 0x1234]);
    assert_eq!((upper.a, upper.a_known), (0x12344567, u32::MAX));

    let mut cleared = partial.clone();
    one(&mut cleared, &mut ports, &[0xa800, 0xafff]);
    assert_eq!((cleared.a, cleared.a_known), (0x80630000, u32::MAX));
    one(&mut cleared, &mut ports, &[0x0043]);
    assert_eq!((cleared.st, cleared.flags_known), (0x8063, FLAGS));
    let mut forced = partial.clone();
    one(&mut forced, &mut ports, &[0xc800, 0x5000]);
    assert_eq!((forced.a, forced.a_known), (0xd0634567, u32::MAX));
    let mut toggled = partial.clone();
    one(&mut toggled, &mut ports, &[0xe800, 0x5000]);
    assert_eq!((toggled.a, toggled.a_known), (partial.a, partial.a_known));
    for opcode in [0xa003, 0xc003] {
        let mut preserved = partial.clone();
        one(&mut preserved, &mut ports, &[opcode]);
        assert_eq!(
            (preserved.a, preserved.a_known),
            (partial.a, partial.a_known)
        );
    }
    for opcode in [0xe003, 0xb700, 0x0037] {
        let mut replaced = partial.clone();
        one(&mut replaced, &mut ports, &[opcode]);
        assert_eq!((replaced.a, replaced.a_known), (0, u32::MAX));
    }
    for (known, value, flags_known) in [
        (0x7fffffff, 1, FLAG_Z),
        (0x7fffffff, 0, 0),
        (0xfffffffe, 0, FLAG_N),
    ] {
        let mut cpu = Core::synthetic(0);
        cpu.a_known = known;
        cpu.a = value;
        one(&mut cpu, &mut ports, &[0xc003]);
        assert_eq!((cpu.st & FLAGS, cpu.flags_known), (0, flags_known));
        roundtrip(&cpu);
    }
    knowledge_states(&partial);
}

fn encoded(cpu: &Core) -> Vec<u8> {
    let mut writer = StateWriter::new();
    cpu.write_state(&mut writer).unwrap();
    writer.into_bytes()
}

fn roundtrip(cpu: &Core) {
    let bytes = encoded(cpu);
    let mut reader = StateReader::new(&bytes);
    assert_eq!(Core::read_state(&mut reader).unwrap(), *cpu);
    assert!(reader.is_exhausted());
}

fn knowledge_states(cpu: &Core) {
    roundtrip(cpu);
    let bytes = encoded(cpu);
    assert_eq!(bytes.len(), 1071);
    assert_eq!(&bytes[1063..1067], b"AKN1");
    for phase in 0..=2 {
        for state in [Core::synthetic(0), cpu.clone()] {
            let mut stream = encoded(&state);
            let end = stream.len();
            stream.push(phase);
            let mut reader = StateReader::new(&stream);
            assert_eq!(Core::read_state(&mut reader).unwrap(), state);
            assert_eq!(reader.position(), end);
            assert_eq!(reader.read_u8().unwrap(), phase);
        }
    }
    for malformed in [
        {
            let mut value = bytes.clone();
            value[7] |= 0x10;
            value
        },
        {
            let mut value = bytes.clone();
            value[1064] = b'?';
            value
        },
        {
            let mut value = bytes.clone();
            value[1067..].copy_from_slice(&u32::MAX.to_le_bytes());
            value
        },
        bytes[..1070].to_vec(),
    ] {
        assert!(Core::read_state(&mut StateReader::new(&malformed)).is_err());
    }
}
