use super::*;

fn witness(
    r: Registers,
    prefetch: [u16; 2],
    code: &[(u32, u16)],
    c: Group0Capture,
    handler: (u32, [u16; 2]),
    proof: ([u16; 7], u64),
) {
    let [opcode, queue] = prefetch;
    let (handler, handler_words) = handler;
    let (frame, duration) = proof;
    let mut cpu = PrefetchCpu::from_prefetched(r, [opcode, queue], CpuClocks(37)).unwrap();
    let mut bus = Device::new(37);
    bus.word(r.pc, opcode);
    bus.word(r.pc + 2, queue);
    for &(address, value) in code {
        bus.word(address, value);
    }
    bus.word(12, (handler >> 16) as u16);
    bus.word(14, handler as u16);
    bus.word(handler, handler_words[0]);
    bus.word(handler.wrapping_add(2), handler_words[1]);
    let retained_address = if opcode == 0x330c { r.pc + 2 } else { r.pc + 4 };
    assert_eq!(bus.sample(retained_address), c.ird);
    assert_eq!(r.a[1].wrapping_sub(2), c.logical_address);
    assert_eq!(c.logical_address & 1, 1);
    bus.detection = Some((Failure::Boundary, 8));
    let initial = bus.words.clone();
    assert_eq!(
        cpu.step_physical_instruction(&mut bus),
        Ok(PhysicalInstructionStep::Group0 {
            opcode,
            capture: c,
            clocks: duration,
        })
    );
    let mut expected = r;
    expected.a[1] = c.logical_address;
    expected.ssp -= 14;
    expected.pc = handler;
    expected.set_status_register(c.phase_sr | SUPERVISOR);
    assert_eq!(cpu.registers, expected);
    assert_eq!(cpu.queued_words(), Some(handler_words));
    assert_eq!(bus.words, oracle::entry_memory(initial, r.ssp, c));
    let mut events = vec![Event::Read(
        oracle::access(r.pc + 4, r.supervisor(), true, AccessKind::Prefetch),
        37,
    )];
    if opcode == 0x333c {
        events.push(Event::Read(
            oracle::access(r.pc + 6, r.supervisor(), true, AccessKind::Prefetch),
            41,
        ));
    }
    let prefix = if opcode == 0x330c { 4 } else { 8 };
    events.push(Event::Detect(
        oracle::access(c.logical_address, r.supervisor(), false, AccessKind::Data),
        Direction::Write,
        37 + prefix,
    ));
    let mut entry = oracle::entry_events(r.ssp, c, 45 + prefix);
    for event in &mut entry {
        if let Event::EntryRead(access, _) = event {
            if access.logical_address == 0x4321_4000 {
                *access = oracle::access(handler, true, true, AccessKind::Prefetch);
            } else if access.logical_address == 0x4321_4002 {
                *access = oracle::access(handler.wrapping_add(2), true, true, AccessKind::Prefetch);
            }
        }
    }
    events.extend(entry);
    assert_eq!(bus.events, events);
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
    assert_eq!(bus.count, prefix as usize / 4);
    assert_eq!(bus.entry_count, 11);
    assert_eq!(cpu.clock().0, 37 + duration);
    assert_eq!(cpu.clock().0, bus.clock);
    assert_eq!(
        cpu.physical_entry_state(),
        PhysicalEntryState::Critical(zeff_m68k::timing::physical::EntryKind::ImportedGroup0)
    );
    assert_eq!(
        cpu.physical_inspection(),
        PhysicalInspection::DiagnosticCompleteValues
    );
}

#[test]
fn literal_move_word_a4_pre_a1_retains_bac4_and_complete_original_frame() {
    let mut r = registers(false, 15);
    r.d = [
        431233508, 1715084032, 4073436260, 2095979144, 2836589133, 2722029598, 3004901459,
        328517080,
    ];
    r.a = [
        3110649824, 3982174657, 497587933, 2683061114, 3049889521, 3129616558, 1751298792,
    ];
    r.usp = 9379628;
    r.ssp = 6532672;
    r.pc = 10807684;
    r.set_status_register(271);
    assert_eq!(r.a[4] as u16, 40689);
    witness(
        r,
        [0x330c, 0xbac4],
        &[(10807688, 63562)],
        Group0Capture {
            vector: 3,
            internal_pc: 10807688,
            phase_sr: 264,
            ird: 0xbac4,
            logical_address: 3982174655,
            function_code: FunctionCode::UserData,
            direction: Direction::Write,
            processing_class: ProcessingClass::InstructionOrGroup2,
            ssw_profile: SswProfile::SelectedIrdUpperBits,
        },
        (2533117692, [2179, 2311]),
        ([59784, 264, 164, 47812, 10687, 47809, 60763], 62),
    );
}

#[test]
fn literal_move_word_immediate_pre_a1_reads_b9fc_and_complete_original_frame() {
    let mut r = registers(true, 0);
    r.d = [
        2110258643, 2719376568, 2301300907, 2650786469, 3153502772, 1183845407, 1357288673,
        1754442915,
    ];
    r.a = [
        3104549076, 3224262825, 1594126317, 447521487, 39248549, 690925663, 3017873027,
    ];
    r.usp = 4880974;
    r.ssp = 1282668;
    r.pc = 10443810;
    r.set_status_register(8448);
    witness(
        r,
        [0x333c, 25537],
        &[(10443814, 0xb9fc), (10443816, 6602)],
        Group0Capture {
            vector: 3,
            internal_pc: 10443816,
            phase_sr: 8448,
            ird: 0xb9fc,
            logical_address: 3224262823,
            function_code: FunctionCode::SupervisorData,
            direction: Direction::Write,
            processing_class: ProcessingClass::InstructionOrGroup2,
            ssw_profile: SswProfile::SelectedIrdUpperBits,
        },
        (2460930876, [28042, 43170]),
        ([23592, 8448, 159, 47612, 22695, 47589, 49198], 66),
    );
}
