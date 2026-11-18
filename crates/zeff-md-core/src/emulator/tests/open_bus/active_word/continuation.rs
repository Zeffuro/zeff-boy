use super::*;

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn busreq_active_word_clone_and_raw_pending_boundary_continuation() {
    let mut hash = Sha256::new();
    let mut cases = 0;
    for phase in [Phase::Crossing, Phase::Pending] {
        for padding in 1..=SOUND_CPU_DIVISOR as usize {
            continue_seed(
                fixture(Region::NtscUs, phase, padding),
                phase,
                false,
                &mut hash,
            );
            cases += 1;
        }
    }
    for absolute in [false, true] {
        let mut matched = 0;
        for padding in 1..=SOUND_CPU_DIVISOR as usize {
            let seed = edge_fixture(padding, absolute);
            let mut probe = seed.clone();
            let capture = Capture::begin().unwrap();
            let result = probe.step_instruction();
            let report = capture.finish(&probe).unwrap();
            let read = busreq_record(&report);
            if !read
                .entry
                .schedule
                .unwrap()
                .pending
                .is_some_and(|pending| pending.deadline == read.service.unwrap().master_ticks + 1)
            {
                continue;
            }
            assert_real_read(&report, &probe, Phase::Pending);
            result.unwrap();
            continue_seed(seed, Phase::Pending, true, &mut hash);
            matched += 1;
            cases += 1;
        }
        assert_eq!(matched, 1);
    }
    emit("busreq-active-continuation-v3", hash, cases);
}

fn core_equal(actual: &Emulator, control: &Emulator) {
    assert_eq!(
        actual.encode_state().unwrap(),
        control.encode_state().unwrap()
    );
    assert_eq!(actual.cpu, control.cpu);
    assert_eq!(actual.sound_cpu, control.sound_cpu);
    assert_eq!(actual.bus.prefetch, control.bus.prefetch);
    assert_eq!(actual.cpu_clock(), control.cpu_clock());
    assert_eq!(actual.timing_snapshot(), control.timing_snapshot());
    assert_eq!(actual.framebuffer(), control.framebuffer());
    assert_eq!(
        actual.framebuffer_dimensions(),
        control.framebuffer_dimensions()
    );
    assert_eq!(actual.video_error(), control.video_error());
}

fn advance_pair(
    actual: &mut Emulator,
    control: &mut Emulator,
    phase: Phase,
    plus_one: bool,
    hash: &mut Sha256,
) {
    let expected = control.step_instruction();
    let capture = Capture::begin().unwrap();
    let result = actual.step_instruction();
    let report = capture.finish(actual).unwrap();
    assert_eq!(result, expected);
    assert_real_read(&report, actual, phase);
    let read = busreq_record(&report);
    if plus_one {
        assert_eq!(
            read.entry.schedule.unwrap().pending.unwrap().deadline,
            read.service.unwrap().master_ticks + 1
        );
    }
    assert_eq!(
        actual.registers().d[0],
        0x1234_0000 | u32::from(value(phase))
    );
    result.unwrap();
    hash.update(format!("{phase:?}{plus_one}{read:?}").as_bytes());
}

fn continue_seed(mut machine: Emulator, phase: Phase, plus_one: bool, hash: &mut Sha256) {
    let deadline = machine.sound_schedule.pending.as_ref().unwrap().deadline;
    assert!(deadline > machine.bus.now.get());
    assert!(machine.bus.sound.requested && !machine.bus.sound.granted);
    let raw = machine.encode_state().unwrap();
    let front = machine.encode_frontend_state().unwrap();
    assert_eq!(
        u32::from_le_bytes(raw[8..12].try_into().unwrap()),
        crate::save_state::STATE_VERSION
    );
    assert_eq!(&front[..8], b"ZMDFRONT");
    assert_eq!(u32::from_le_bytes(front[8..12].try_into().unwrap()), 1);
    let mut cloned = machine.clone();
    let mut raw_actual = machine.clone();
    let mut raw_control = machine.clone();
    let mut front_actual = machine.clone();
    let mut front_control = machine.clone();
    for current in [&mut raw_actual, &mut raw_control] {
        current.decode_state(&raw).unwrap();
        assert_eq!(current.encode_state().unwrap(), raw);
        assert_eq!(
            current.sound_schedule.pending.as_ref().unwrap().deadline,
            deadline
        );
        core_equal(current, &machine);
    }
    for current in [&mut front_actual, &mut front_control] {
        current.decode_frontend_state(&front).unwrap();
        assert_eq!(current.encode_frontend_state().unwrap(), front);
        assert_eq!(
            current.sound_schedule.pending.as_ref().unwrap().deadline,
            deadline
        );
        assert_eq!(*current, machine);
    }
    advance_pair(&mut machine, &mut cloned, phase, plus_one, hash);
    advance_pair(&mut raw_actual, &mut raw_control, phase, plus_one, hash);
    advance_pair(&mut front_actual, &mut front_control, phase, plus_one, hash);
    core_equal(&raw_actual, &machine);
    core_equal(&raw_control, &machine);
    assert_eq!(machine, front_actual);
    assert_eq!(machine, front_control);
    compare(&mut machine, &mut cloned, hash);
    compare(&mut raw_actual, &mut raw_control, hash);
    compare(&mut front_actual, &mut front_control, hash);
    assert_eq!(machine, front_actual);
}

fn edge_fixture(padding: usize, absolute: bool) -> Emulator {
    let mut program = vec![0x4e71_u16; padding];
    program.extend([0x34bc, 0x0100]);
    if absolute {
        program.extend([0x3039, 0x00a1, 0x1100]);
    } else {
        program.extend([0x4e71, 0x3012]);
    }
    program.extend([0x0240, 0xffff, 0x60fe, 0x4e71]);
    let mut image = vec![0; 0x200];
    image[..8].copy_from_slice(&[0, 0xff, 0xff, 0xfc, 0, 0, 1, 0]);
    for (index, word) in program.iter().enumerate() {
        image[0x100 + index * 2..0x102 + index * 2].copy_from_slice(&word.to_be_bytes());
    }
    let mut machine = Emulator::new(
        image,
        LoadConfig {
            region: Region::NtscUs,
            concurrent_sound: true,
        },
    )
    .unwrap();
    machine.cpu.registers.a[2] = SOUND_BUS_REQUEST;
    machine.cpu.registers.d[0] = 0x1234_beef;
    for _ in 0..padding {
        machine.step_instruction().unwrap();
    }
    machine.bus.sound.set_reset(false);
    let sound: &[u8] = if absolute {
        &[0x18, 0, 0x76]
    } else {
        &[0x01, 0x34, 0x12, 0x76]
    };
    machine.bus.sound.ram[..sound.len()].copy_from_slice(sound);
    let pending = PendingSound::prepare(
        &machine.sound_cpu,
        &machine.bus.sound,
        &machine.bus.cartridge,
        machine.bus.now.get(),
    )
    .unwrap();
    assert!(!pending.any_bank_access);
    assert!(pending.bank_access.is_none());
    assert!(pending.reservation().is_none());
    machine.sound_schedule.pending = Some(pending);
    let capture = Capture::begin().unwrap();
    machine.step_instruction().unwrap();
    if !absolute {
        machine.step_instruction().unwrap();
    }
    let report = capture.finish(&machine).unwrap();
    assert!(report.complete);
    assert!(report.history.iter().flatten().any(|record| {
        record.callback == Callback::Write
            && record.write_value == Some(0x0100)
            && record
                .access
                .is_some_and(|access| access.bus_address == SOUND_BUS_REQUEST)
    }));
    if !absolute {
        assert!(report.history.iter().flatten().any(|record| {
            record.access.is_some_and(|access| {
                access.kind == AccessKind::Prefetch && access.lanes == ByteLanes::Both
            }) && record.outcome == Some(Outcome::Read(0x0240))
                && record.returned.unwrap().bus.prefetch == Observation::Full(0x0240)
        }));
    }
    assert_eq!(
        machine.queued_words().unwrap()[0],
        if absolute { 0x3039 } else { 0x3012 }
    );
    assert!(machine.bus.sound.requested);
    assert!(!machine.bus.sound.granted);
    machine
}

fn exact_pending_edge(absolute: bool) {
    let mut hash = Sha256::new();
    let mut matched = 0;
    for padding in 1..=SOUND_CPU_DIVISOR as usize {
        let mut actual = edge_fixture(padding, absolute);
        let mut control = actual.clone();
        let expected = control.step_instruction();
        let capture = Capture::begin().unwrap();
        let result = actual.step_instruction();
        let report = capture.finish(&actual).unwrap();
        assert_eq!(result, expected);
        assert!(report.complete);
        let read = busreq_record(&report);
        let service = read.service.unwrap();
        let pending = read.entry.schedule.unwrap().pending;
        if !pending.is_some_and(|pending| pending.deadline == service.master_ticks + 1) {
            continue;
        }
        matched += 1;
        if !matches!(read.outcome, Some(Outcome::Read(_))) {
            emit_record(Phase::Pending, padding, read);
            assert!(matches!(result, Err(MachineFault::Main(TimingFault::Bus {
                access,
                direction: Direction::Read,
                source: BusError::UnsupportedAccess { address: SOUND_BUS_REQUEST, direction: Direction::Read },
            })) if access == super::access(SOUND_BUS_REQUEST, AccessKind::Data, ByteLanes::Both)));
            assert_eq!(actual.registers().d[0], 0x1234_beef);
        }
        let records: Vec<_> = report.history.iter().flatten().collect();
        let index = records
            .iter()
            .position(|record| record.sequence == read.sequence)
            .unwrap();
        if absolute {
            let fetched = records[index - 1];
            assert_eq!(fetched.access.unwrap().kind, AccessKind::Prefetch);
            assert_eq!(fetched.access.unwrap().lanes, ByteLanes::Both);
            assert_eq!(fetched.outcome, Some(Outcome::Read(0x0240)));
            assert_eq!(
                fetched.returned.unwrap().bus.prefetch,
                Observation::Full(0x0240)
            );
        } else {
            assert_eq!(read.entry.bus.prefetch, Observation::Full(0x0240));
        }
        assert_real_read(&report, &actual, Phase::Pending);
        result.unwrap();
        assert_eq!(actual.registers().d[0], 0x1234_0340);
        compare(&mut actual, &mut control, &mut hash);
        hash.update(format!("{absolute}{padding}{read:?}").as_bytes());
    }
    assert_eq!(matched, 1);
    emit(
        if absolute {
            "busreq-active-absolute-edge-v2"
        } else {
            "busreq-active-indirect-edge-v2"
        },
        hash,
        matched,
    );
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn busreq_active_word_indirect_service_one_tick_before_deadline() {
    exact_pending_edge(false);
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn busreq_active_word_absolute_service_one_tick_before_deadline() {
    exact_pending_edge(true);
}
