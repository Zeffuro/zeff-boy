use super::*;

#[test]
fn literal_move_captured_tst_word_d5_now_has_its_source_proven_complete_frame() {
    let mut r = registers(false, 29);
    r.pc = 6_321_690;
    r.a[1] = 4_195_431_093;
    r.a[3] = 667_211_460;
    r.usp = 16_247_738;
    r.ssp = 2_262_118;
    r.set_status_register(285);
    let mut cpu = PrefetchCpu::from_prefetched(r, [0x332b, 61_308], CpuClocks(37)).unwrap();
    let mut bus = Device::new(37);
    for (address, value) in [
        (r.pc, 0x332b),
        (r.pc + 2, 61_308),
        (r.pc + 4, 0x4a45),
        (r.pc + 6, 0xf1c6),
        (12_895_808, 636),
        (12, 0x5a52),
        (14, 0x0c84),
        (0x5a52_0c84, 0x83ad),
        (0x5a52_0c86, 0xa93b),
    ] {
        bus.word(address, value);
    }
    assert_eq!(
        r.a[3].wrapping_add_signed(i32::from(61_308u16 as i16)) & BUS_ADDRESS_MASK,
        12_895_808
    );
    assert_eq!(bus.sample(r.pc + 4), 0x4a45);
    bus.detection = Some((Failure::Boundary, 8));
    let initial = bus.words.clone();
    let c = Group0Capture {
        vector: 3,
        internal_pc: 6_321_696,
        phase_sr: 272,
        ird: 0x4a45,
        logical_address: 4_195_431_091,
        function_code: FunctionCode::UserData,
        direction: Direction::Write,
        processing_class: ProcessingClass::InstructionOrGroup2,
        ssw_profile: SswProfile::SelectedIrdUpperBits,
    };
    assert_eq!(
        cpu.step_physical_instruction(&mut bus),
        Ok(PhysicalInstructionStep::Group0 {
            opcode: 0x332b,
            capture: c,
            clocks: 70
        })
    );
    let mut expected = r;
    expected.a[1] = 4_195_431_091;
    expected.ssp -= 14;
    expected.pc = 0x5a52_0c84;
    expected.set_status_register(272 | SUPERVISOR);
    assert_eq!(cpu.registers, expected);
    assert_eq!(cpu.queued_words(), Some([0x83ad, 0xa93b]));
    let mut events = vec![
        Event::Read(
            oracle::access(r.pc + 4, false, true, AccessKind::Prefetch),
            37,
        ),
        Event::Read(
            oracle::access(667_207_232, false, false, AccessKind::Data),
            41,
        ),
        Event::Read(
            oracle::access(r.pc + 6, false, true, AccessKind::Prefetch),
            45,
        ),
        Event::Detect(
            oracle::access(4_195_431_091, false, false, AccessKind::Data),
            Direction::Write,
            49,
        ),
    ];
    let mut entry = oracle::entry_events(r.ssp, c, 57);
    for event in &mut entry {
        if let Event::EntryRead(a, _) = event {
            if a.logical_address == 0x4321_4000 {
                *a = oracle::access(0x5a52_0c84, true, true, AccessKind::Prefetch);
            } else if a.logical_address == 0x4321_4002 {
                *a = oracle::access(0x5a52_0c86, true, true, AccessKind::Prefetch);
            }
        }
    }
    events.extend(entry);
    assert_eq!(bus.events, events);
    assert_eq!(bus.words, oracle::entry_memory(initial, r.ssp, c));
    assert_eq!(
        [
            bus.sample(r.ssp - 2),
            bus.sample(r.ssp - 6),
            bus.sample(r.ssp - 4),
            bus.sample(r.ssp - 8),
            bus.sample(r.ssp - 10),
            bus.sample(r.ssp - 14),
            bus.sample(r.ssp - 12)
        ],
        [30240, 272, 96, 19013, 12979, 19009, 64017]
    );
    assert_eq!(bus.count, 3);
    assert_eq!(bus.entry_count, 11);
    assert_eq!(cpu.clock().0, 107);
    assert_eq!(cpu.clock().0, bus.clock);
}

#[test]
fn processing_classifier_recognizes_all_150_legal_tst_encodings_without_execution_admission() {
    let mut recognized = 0;
    for size in [Size::Byte, Size::Word, Size::Long] {
        let size_bits = match size {
            Size::Byte => 0,
            Size::Word => 0x40,
            Size::Long => 0x80,
        };
        let dn = (0..8).map(|register| 0x4a00 | size_bits | register);
        let memory = EAS.into_iter().flat_map(|ea| {
            (0..if matches!(ea, Ea::AbsoluteWord | Ea::AbsoluteLong) {
                1
            } else {
                8
            })
                .map(move |register| Form { size, ea }.opcode(register))
        });
        for opcode in dn.chain(memory) {
            let mut case = Case::new(Form {
                size: Size::Long,
                ea: Ea::Post,
            });
            case.next = opcode;
            capture::failed(case, 2, Failure::Boundary);
            recognized += 1;
        }
    }
    assert_eq!(recognized, 150);
}
