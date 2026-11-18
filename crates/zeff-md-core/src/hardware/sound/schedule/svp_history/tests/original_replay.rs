use super::*;

fn changes(kind: u8) -> Vec<CheatPatch> {
    match kind {
        0 => vec![patch(0x1001, 1)],
        1 => vec![patch(0x1000, 0)],
        2 => vec![patch(0x1000, 0), patch(0x1001, 1)],
        _ => vec![],
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn whole_all_original_rom_inputs_survive_peripheral_refresh_after_opcode_or_operand_mask() {
    for unrelated in [false, true] {
        let mut cart = fixture(&[0x3a, 0, 0x40, 0x76]);
        if unrelated {
            cart.rom_overlay = cart.prepare_rom_overlay(&[patch(0x1100, 0xa5)]).unwrap();
        }
        let cpu = Cpu::new_with_reset(zeff_z80::ResetState::new(0x9000, 0x1ffe));
        let mut memory = memory();
        memory.ram[0x200] = 0x33;
        let mut pending = PendingSound::prepare(&cpu, &memory, &cart, 300).unwrap();
        assert!(pending.replay.is_some());
        assert_eq!(pending.cpu.regs().a, 0);
        let history = pending.svp_history.as_ref().unwrap();
        assert_eq!(history.reads.len(), 3);
        assert!(
            history
                .reads
                .iter()
                .all(|read| read.original == read.effective)
        );
        assert_eq!(
            history
                .reads
                .iter()
                .map(|read| (read.ordinal, read.local, read.offset, read.effective))
                .collect::<Vec<_>>(),
            vec![
                (1, 0x9000, 0x1000, 0x3a),
                (2, 0x9001, 0x1001, 0),
                (3, 0x9002, 0x1002, 0x40)
            ]
        );
        assert_eq!(history.source_memory.ram[0x200], 0x33);
        pending
            .reserve_after(&[Reservation {
                requester: Requester::VDP,
                ticks: 300..600,
            }])
            .unwrap();
        let interval = (pending.start, pending.deadline);
        assert_eq!(interval.0, 600);
        let mut audio = Audio::default();
        audio.write_opn2(300, 0, 0x2a);
        audio.write_opn2(300, 1, 0x80);
        let expected = audio.opn2_status_at(pending.deadline);
        assert_ne!(expected & 0x80, 0);
        for kind in 0..4 {
            cart.rom_overlay = cart.prepare_rom_overlay(&changes(kind)).unwrap();
            let overlay = cart.rom_overlay.clone();
            let mut restored = restore(&pending, &cpu, &cart).pending.unwrap();
            let mut live_memory = memory.clone();
            live_memory.ram[0x200] = 0x66;
            restored
                .refresh_reads(&cpu, &live_memory, &cart, &audio)
                .unwrap();
            assert_eq!(cart.rom_overlay, overlay);
            assert_eq!((restored.start, restored.deadline), interval);
            assert_eq!(restored.cpu.regs().a, expected);
            assert_eq!(restored.instruction.pc, 0x9000);
            assert_eq!(restored.instruction.opcode, 0x3a);
            assert!(restored.replay.is_none());
            assert!(restored.svp_history.as_ref().unwrap().reads_settled);
            assert_eq!(restored.memory.ram[0x200], 0x33);
            assert_eq!(live_memory.ram[0x200], 0x66);
            restore(&restored, &cpu, &cart);
            let mut next_cpu = cpu.clone();
            let mut audio = audio.clone();
            restored.publish(
                &mut next_cpu,
                &mut live_memory,
                &mut [],
                &mut audio,
                &mut cart,
            );
            assert_eq!(next_cpu.regs().a, expected);
            assert_eq!(next_cpu.regs().pc, 0x9003);
            assert_eq!(live_memory.ram[0x200], 0x33);
        }
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn all_original_refresh_witness_stays_strict_and_unneeded_history_stays_absent() {
    let cart = fixture(&[0x3a, 0, 0x40]);
    let cpu = Cpu::new_with_reset(zeff_z80::ResetState::new(0x9000, 0x1ffe));
    let memory = memory();
    let pending = PendingSound::prepare(&cpu, &memory, &cart, 300).unwrap();
    for change in 0..8 {
        let mut bad = pending.clone();
        let history = bad.svp_history.as_mut().unwrap();
        match change {
            0 => {
                history.reads.remove(1);
            }
            1 => history.reads.swap(0, 1),
            2 => history.reads.push(history.reads[0]),
            3 => history.reads[0].original ^= 1,
            4 => history.reads[0].ordinal += 1,
            5 => {
                bad.replay = None;
            }
            6 => history.reads_settled = true,
            _ => {
                let mut extra = history.reads[2];
                extra.ordinal = 5;
                extra.local = 0x9004;
                extra.offset = 0x1004;
                extra.original = cart.bytes[0x1004];
                extra.effective = extra.original;
                history.reads.push(extra);
            }
        }
        assert!(
            bad.validate_history(&cpu, BusState::default(), &cart)
                .is_err()
        );
    }
    let plain_cart = fixture(&[0x3e, 0x11]);
    let plain = PendingSound::prepare(&cpu, &memory, &plain_cart, 300).unwrap();
    assert!(plain.replay.is_none() && plain.svp_history.is_none());
    let schedule = SoundSchedule {
        pending: Some(plain),
        ..Default::default()
    };
    assert!(schedule.write_svp_history(&cpu).unwrap().is_empty());
    let mut w = StateWriter::new();
    schedule.write_state(&cpu, &mut w).unwrap();
    let bytes = w.into_bytes();
    let mut restored_cpu = cpu.clone();
    let mut restored = SoundSchedule::default();
    restored
        .read_state(&mut restored_cpu, &mut StateReader::new(&bytes))
        .unwrap();
    let mut out = StateWriter::new();
    restored.write_state(&restored_cpu, &mut out).unwrap();
    assert_eq!(out.into_bytes(), bytes);
    assert_eq!(restored, schedule);
}
