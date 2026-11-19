use super::*;

#[test]
fn golden_raw_adda_word_pre_firstodd_retains_full_frame_actual_handler_and_whole_sr() {
    let mut r = Registers::new();
    r.d = [
        1759828243, 507908270, 4141399671, 790521190, 2010804182, 2302680764, 4273594614,
        3558615520,
    ];
    r.a = [
        4184147031, 4102325717, 3753529812, 2071599576, 1495717915, 254845233, 3391612766,
    ];
    r.usp = 10314770;
    r.ssp = 3228916;
    r.pc = 6867594;
    r.set_status_register(9494);
    let mut cpu = PrefetchCpu::from_prefetched(r, [0xdae5, 28664], CpuClocks(0)).unwrap();
    let mut bus = Device::new(0);
    let target = 1930218814;
    for (a, v) in [
        (r.pc, 0xdae5),
        (r.pc + 2, 28664),
        (12, 29452),
        (14, 52542),
        (target, 33550),
        (target + 2, 54527),
    ] {
        bus.word(a, v);
    }
    bus.detection = Some((Failure::Boundary, 8));
    let initial = bus.words.clone();
    let c = Group0Capture {
        vector: 3,
        internal_pc: 6867598,
        phase_sr: 9494,
        ird: 0xdae5,
        logical_address: 254845231,
        function_code: FunctionCode::SupervisorData,
        direction: Direction::Read,
        processing_class: ProcessingClass::InstructionOrGroup2,
        ssw_profile: SswProfile::SelectedIrdUpperBits,
    };
    assert_eq!(c.special_status_word(), 56053);
    assert_eq!(
        cpu.step_physical_instruction(&mut bus),
        Ok(PhysicalInstructionStep::Group0 {
            opcode: 0xdae5,
            capture: c,
            clocks: 60
        })
    );
    let mut expected = r;
    expected.a[5] = 254845231;
    expected.ssp -= 14;
    expected.pc = target;
    assert_eq!(cpu.registers, expected);
    assert_eq!(cpu.queued_words(), Some([33550, 54527]));
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
        [51854, 9494, 104, 56037, 41263, 56053, 3888]
    );
    let mut events = vec![
        Event::Idle(0, 2),
        Event::Detect(
            oracle::access(254845231, true, false, AccessKind::Data),
            Direction::Read,
            2,
        ),
    ];
    let mut suffix = oracle::entry_events(r.ssp, c, 10);
    for e in &mut suffix {
        if let Event::EntryRead(a, _) = e {
            if a.logical_address == 0x4321_4000 {
                *a = oracle::access(target, true, true, AccessKind::Prefetch);
            } else if a.logical_address == 0x4321_4002 {
                *a = oracle::access(target + 2, true, true, AccessKind::Prefetch);
            }
        }
    }
    events.extend(suffix);
    assert_eq!(bus.events, events);
    assert_eq!(bus.words, oracle::entry_memory(initial, r.ssp, c));
    assert_eq!(
        (cpu.clock().0, bus.clock, bus.count, bus.entry_count),
        (60, 60, 0, 11)
    );
}

#[test]
fn golden_raw_adda_long_post_same_an_uses_actual_ram_updated_destination_and_trailing_i2() {
    let mut r = Registers::new();
    r.d = [
        365607540, 3165444630, 678001591, 3542391576, 2701463833, 3630402718, 1061197158, 874950165,
    ];
    r.a = [
        2282453523, 1103685791, 1328832467, 2799232028, 3195367412, 1880770602, 2894130793,
    ];
    r.usp = 7156526;
    r.ssp = 2312524;
    r.pc = 6017526;
    r.set_status_register(4);
    let mut cpu = PrefetchCpu::from_prefetched(r, [0xdbdd, 4607], CpuClocks(0)).unwrap();
    let mut bus = Device::new(0);
    for (a, v) in [
        (r.pc, 0xdbdd),
        (r.pc + 2, 4607),
        (r.pc + 4, 20718),
        (1880770602, 55359),
        (1880770604, 51254),
    ] {
        bus.word(a, v);
    }
    assert_eq!(
        u32::from(bus.sample(1880770602)) << 16 | u32::from(bus.sample(1880770604)),
        3628058678
    );
    let initial = bus.words.clone();
    let case = Case {
        r,
        register: 5,
        destination: 5,
        value: 3628058678,
        start: 0,
        next: 4607,
        last: 20718,
        ..Case::new(Form {
            operation: Operation::Add,
            size: Size::Long,
            ea: Ea::Post,
        })
    };
    let plan = oracle::plan(case, &initial);
    assert_eq!(
        plan.iter().map(|t| t.registers.a[5]).collect::<Vec<_>>(),
        [1880770602, 1880770606, 1880756324]
    );
    assert_eq!(oracle::arithmetic(case, &initial), (1880756324, 1213861988));
    assert_ne!(r.a[5].wrapping_add(3628058678), 1213861988);
    assert_eq!(
        cpu.step_physical_instruction(&mut bus),
        Ok(PhysicalInstructionStep::Instruction(
            zeff_m68k::timing::TimedStep {
                opcode: 0xdbdd,
                clocks: 14,
                exception: None
            }
        ))
    );
    let mut expected = r;
    expected.a[5] = 1213861988;
    expected.pc += 2;
    assert_eq!(cpu.registers, expected);
    assert_eq!(cpu.queued_words(), Some([4607, 20718]));
    assert_eq!(
        bus.events,
        vec![
            Event::Read(
                oracle::access(1880770602, false, false, AccessKind::Data),
                0
            ),
            Event::Read(
                oracle::access(1880770604, false, false, AccessKind::Data),
                4
            ),
            Event::Read(
                oracle::access(6017530, false, true, AccessKind::Prefetch),
                8
            ),
            Event::Idle(12, 2)
        ]
    );
    assert_eq!(bus.words, initial);
    assert_eq!((cpu.clock().0, bus.clock, bus.count), (14, 14, 3));
}
