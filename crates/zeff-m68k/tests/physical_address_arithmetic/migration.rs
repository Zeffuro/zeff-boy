use super::*;

fn original(
    initial: (Registers, [u16; 2]),
    ram: &[(u32, u16)],
    expected: Registers,
    c: Group0Capture,
    handler_words: [u16; 2],
    proof: ([u16; 7], u64),
) {
    let (r, queue) = initial;
    let (frame, duration) = proof;
    let mut cpu = PrefetchCpu::from_prefetched(r, queue, CpuClocks(0)).unwrap();
    let mut bus = Device::new(0);
    for &(address, word) in ram {
        bus.word(address, word);
    }
    assert_eq!(bus.sample(r.pc), queue[0]);
    assert_eq!(bus.sample(r.pc + 2), c.ird);
    let initial = bus.words.clone();
    let mode = queue[0] >> 3 & 7;
    let register = usize::from(queue[0] & 7);
    let mut events = Vec::new();
    let mut clock = 0;
    if mode == 4 {
        events.push(Event::Idle(0, 2));
        clock = 2;
    }
    if mode != 1 {
        let base = r.address_register(register).unwrap();
        let source = if mode == 4 {
            base.wrapping_sub(2)
        } else {
            base
        };
        assert_eq!(source & 1, 0);
        events.push(Event::Read(
            oracle::access(source, r.supervisor(), false, AccessKind::Data),
            clock,
        ));
        clock += 4;
    }
    events.push(Event::Read(
        oracle::access(r.pc + 4, r.supervisor(), true, AccessKind::Prefetch),
        clock,
    ));
    clock += 4;
    events.push(Event::Detect(
        oracle::access(c.logical_address, r.supervisor(), false, AccessKind::Data),
        Direction::Write,
        clock,
    ));
    bus.detection = Some((Failure::Boundary, 8));
    assert_eq!(
        cpu.step_physical_instruction(&mut bus),
        Ok(PhysicalInstructionStep::Group0 {
            opcode: queue[0],
            capture: c,
            clocks: duration
        })
    );
    let mut entry = oracle::entry_events(r.ssp, c, clock + 8);
    for event in &mut entry {
        if let Event::EntryRead(a, _) = event {
            if a.logical_address == 0x43214000 {
                *a = oracle::access(expected.pc, true, true, AccessKind::Prefetch);
            } else if a.logical_address == 0x43214002 {
                *a = oracle::access(expected.pc + 2, true, true, AccessKind::Prefetch);
            }
        }
    }
    events.extend(entry);
    assert_eq!(cpu.registers, expected);
    assert_eq!(cpu.queued_words(), Some(handler_words));
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
        frame
    );
    assert_eq!(bus.count, (clock - u64::from(mode == 4) * 2) as usize / 4);
    assert_eq!(bus.entry_count, 11);
    assert_eq!((cpu.clock().0, bus.clock), (duration, duration));
    assert_eq!(
        cpu.physical_inspection(),
        PhysicalInspection::DiagnosticCompleteValues
    );
}

#[test]
fn original_move_319_retained_address_arithmetic_word_has_full_frame_and_state() {
    let mut r = Registers::new();
    r.d = [
        553289367, 3576893734, 1806129312, 3820921422, 3835287540, 3703389922, 3143773972,
        1088137821,
    ];
    r.a = [
        1809609563, 56708608, 2155781718, 949448312, 1159184154, 2657030534, 1895660515,
    ];
    r.usp = 4535612;
    r.ssp = 9098432;
    r.pc = 12345528;
    r.set_status_register(9992);
    let mut expected = Registers::new();
    expected.d = [
        553289367, 3576893734, 1806129312, 3820921422, 3835287540, 3703389922, 3143773972,
        1088137821,
    ];
    expected.a = [
        1809609563, 56708608, 2155781716, 949448312, 1159184154, 2657030534, 1895660513,
    ];
    expected.usp = 4535612;
    expected.ssp = 9098418;
    expected.pc = 28492286;
    expected.set_status_register(9992);
    original(
        (r, [15650, 40686]),
        &[
            (12, 434),
            (14, 49662),
            (8298068, 60116),
            (11715070, 50398),
            (11715072, 26935),
            (12345528, 15650),
            (12345530, 40686),
            (12345532, 59869),
        ],
        expected,
        Group0Capture {
            vector: 3,
            internal_pc: 12345532,
            phase_sr: 9992,
            ird: 40686,
            logical_address: 1895660513,
            function_code: FunctionCode::SupervisorData,
            direction: Direction::Write,
            processing_class: ProcessingClass::InstructionOrGroup2,
            ssw_profile: SswProfile::SelectedIrdUpperBits,
        },
        [50398, 26935],
        ([24764, 9992, 188, 40686, 31713, 40677, 28925], 68),
    );
}

#[test]
fn original_move_553_retained_address_arithmetic_word_has_full_frame_and_state() {
    let mut r = Registers::new();
    r.d = [
        897020176, 759940340, 2042485027, 3427603436, 866346442, 526739418, 3924025670, 3792317144,
    ];
    r.a = [
        782811415, 659243276, 1565530716, 2107446777, 2814319841, 3218127661, 4197297190,
    ];
    r.usp = 7739906;
    r.ssp = 16666074;
    r.pc = 7701254;
    r.set_status_register(1540);
    let mut expected = Registers::new();
    expected.d = [
        897020176, 759940340, 2042485027, 3427603436, 866346442, 526739418, 3924025670, 3792317144,
    ];
    expected.a = [
        782811415, 659243276, 1565530716, 2107446777, 2814319841, 3218127659, 4197297190,
    ];
    expected.usp = 7739906;
    expected.ssp = 16666060;
    expected.pc = 4052628724;
    expected.set_status_register(9736);
    original(
        (r, [15118, 55767]),
        &[
            (12, 61838),
            (14, 13556),
            (7701254, 15118),
            (7701256, 55767),
            (7701258, 50784),
            (9319668, 9266),
            (9319670, 46073),
        ],
        expected,
        Group0Capture {
            vector: 3,
            internal_pc: 7701258,
            phase_sr: 1544,
            ird: 55767,
            logical_address: 3218127659,
            function_code: FunctionCode::UserData,
            direction: Direction::Write,
            processing_class: ProcessingClass::InstructionOrGroup2,
            ssw_profile: SswProfile::SelectedIrdUpperBits,
        },
        [9266, 46073],
        ([33546, 1544, 117, 55767, 47915, 55745, 49104], 62),
    );
}

#[test]
fn original_move_1924_retained_address_arithmetic_word_has_full_frame_and_state() {
    let mut r = Registers::new();
    r.d = [
        1465541015, 4264807041, 2912571684, 1298400955, 4193304163, 1425898854, 3004399321,
        612738383,
    ];
    r.a = [
        732277797, 1951457428, 3163246425, 3690730315, 449518453, 2052325892, 1500485633,
    ];
    r.usp = 11175184;
    r.ssp = 1865704;
    r.pc = 4033700;
    r.set_status_register(26);
    let mut expected = Registers::new();
    expected.d = [
        1465541015, 4264807041, 2912571684, 1298400955, 4193304163, 1425898854, 3004399321,
        612738383,
    ];
    expected.a = [
        732277797, 1951457428, 3163246423, 3690730315, 449518453, 2052325892, 1500485633,
    ];
    expected.usp = 11175184;
    expected.ssp = 1865690;
    expected.pc = 1381256894;
    expected.set_status_register(8208);
    original(
        (r, [13591, 56562]),
        &[
            (12, 21076),
            (14, 20158),
            (4033700, 13591),
            (4033702, 56562),
            (4033704, 27734),
            (5525182, 59010),
            (5525184, 34720),
            (11175184, 10968),
        ],
        expected,
        Group0Capture {
            vector: 3,
            internal_pc: 4033704,
            phase_sr: 16,
            ird: 56562,
            logical_address: 3163246423,
            function_code: FunctionCode::UserData,
            direction: Direction::Write,
            processing_class: ProcessingClass::InstructionOrGroup2,
            ssw_profile: SswProfile::SelectedIrdUpperBits,
        },
        [59010, 34720],
        ([36008, 16, 61, 56562, 20311, 56545, 48267], 66),
    );
}

#[test]
fn original_move_2299_retained_address_arithmetic_word_has_full_frame_and_state() {
    let mut r = Registers::new();
    r.d = [
        3796605601, 621277735, 2079074132, 3441372440, 1468988114, 3392886465, 2656458082,
        1734958135,
    ];
    r.a = [
        61769343, 4077874927, 2991694340, 3945059474, 2084865678, 3668079451, 3038398289,
    ];
    r.usp = 11869000;
    r.ssp = 12088838;
    r.pc = 4438726;
    r.set_status_register(8713);
    let mut expected = Registers::new();
    expected.d = [
        3796605601, 621277735, 2079074132, 3441372440, 1468988114, 3392886465, 2656458082,
        1734958135,
    ];
    expected.a = [
        61769343, 4077874927, 2991694340, 3945059474, 2084865678, 3668079451, 3038398287,
    ];
    expected.usp = 11869000;
    expected.ssp = 12088824;
    expected.pc = 1715653266;
    expected.set_status_register(8704);
    original(
        (r, [15639, 57328]),
        &[
            (12, 26178),
            (14, 51858),
            (4377234, 64692),
            (4377236, 23845),
            (4438726, 15639),
            (4438728, 57328),
            (4438730, 61571),
            (12088838, 19487),
        ],
        expected,
        Group0Capture {
            vector: 3,
            internal_pc: 4438730,
            phase_sr: 8704,
            ird: 57328,
            logical_address: 3038398287,
            function_code: FunctionCode::SupervisorData,
            direction: Direction::Write,
            processing_class: ProcessingClass::InstructionOrGroup2,
            ssw_profile: SswProfile::SelectedIrdUpperBits,
        },
        [64692, 23845],
        ([47818, 8704, 67, 57328, 18255, 57317, 46362], 66),
    );
}
