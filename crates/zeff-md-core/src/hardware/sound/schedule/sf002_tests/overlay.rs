use super::*;
use zeff_emu_common::cheats::{CheatPatch, CheatValue};

fn install(cart: &mut Cartridge, values: &[(u32, u8)]) {
    let patches: Vec<_> = values
        .iter()
        .map(|&(address, value)| CheatPatch::WideRomWrite {
            address,
            value: CheatValue::Constant(value),
        })
        .collect();
    cart.rom_overlay = cart.prepare_rom_overlay(&patches).unwrap();
}

fn reach(
    cpu: &mut Cpu,
    memory: &mut SoundMemory,
    cart: &mut Cartridge,
    schedule: &mut SoundSchedule,
    timeline: (&mut Audio, &mut u64),
    target: u32,
) {
    let (audio, now) = timeline;
    for _ in 0..3000 {
        schedule.advance(
            cpu,
            memory,
            cart,
            &mut [],
            SoundInterval::new(*now..*now + 1, audio),
        );
        *now += 1;
        assert_eq!(schedule.fault, None);
        if schedule.pending.as_ref().is_some_and(|p| {
            p.bank_access
                .is_some_and(|a| a.address == target && a.direction == Direction::Read)
        }) {
            return;
        }
    }
    panic!("SOUND read not reached {target:06x}");
}

fn retire(
    cpu: &mut Cpu,
    memory: &mut SoundMemory,
    cart: &mut Cartridge,
    audio: &mut Audio,
    schedule: &mut SoundSchedule,
    now: &mut u64,
) {
    let end = schedule.pending.as_ref().unwrap().deadline;
    assert!(end > *now);
    schedule.advance(
        cpu,
        memory,
        cart,
        &mut [],
        SoundInterval::new(*now..end, audio),
    );
    *now = end;
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn sf002_overlay_sound_journal_resolves_projected_mapping_before_source_overlay() {
    for mapping in [Mapping::Unknown, Mapping::Linear, Mapping::Mirrored] {
        let mut cart = cartridge();
        if mapping != Mapping::Unknown {
            assert!(cart.sf002.as_mut().unwrap().control(access(0xe01), 0));
        }
        if mapping == Mapping::Mirrored {
            assert!(cart.sf002.as_mut().unwrap().control(access(0xe01), 0x81));
        }
        install(&mut cart, &[(0x796c, 0xa5), (0x20796c, 0x5a)]);
        let before = cart.clone();
        for (bank, expected) in [
            (0, Some(0xa5)),
            (
                0x40,
                match mapping {
                    Mapping::Unknown => None,
                    Mapping::Linear => Some(0x5a),
                    Mapping::Mirrored => Some(0xa5),
                },
            ),
        ] {
            let mut memory = SoundMemory {
                bank,
                ..Default::default()
            };
            let candidate = CandidateBus::new(&mut memory, &cart);
            let value = candidate.cpu_read(0xf96c);
            match expected {
                Some(expected) => {
                    assert_eq!(value, expected);
                    assert_eq!(candidate.fault.get(), None);
                }
                None => assert!(matches!(
                    candidate.fault.get(),
                    Some(SoundFault::UnsupportedAccess {
                        direction: Direction::Read,
                        ..
                    })
                )),
            }
        }
        let mut memory = SoundMemory::default();
        let mut candidate = CandidateBus::new(&mut memory, &cart);
        candidate.cpu_write(0x8e01, 0);
        candidate.memory.bank = 0x40;
        assert_eq!(candidate.cpu_read(0xf96c), 0x5a);
        candidate.memory.bank = 0;
        candidate.cpu_write(0x8e01, 0x81);
        candidate.memory.bank = 0x40;
        assert_eq!(candidate.cpu_read(0xf96c), 0xa5);
        assert_eq!(candidate.fault.get(), None);
        assert_eq!(cart, before);
        let journal = candidate.sf002_writes;
        journal.project(&mut cart).unwrap();
        assert_eq!(cart.read(0x20796c), Some(0xa5));
        let mut memory = SoundMemory {
            bank: 0x78,
            ..Default::default()
        };
        let mut candidate = CandidateBus::new(&mut memory, &cart);
        candidate.cpu_write(0x8001, 0x42);
        assert_eq!(candidate.cpu_read(0x8001), 0x42);
        assert_eq!(candidate.fault.get(), None);
        assert_eq!(cart.read(0x3c0001), Some(0xff));
        let journal = candidate.sf002_writes;
        journal.publish(&mut cart);
        assert_eq!(cart.read(0x3c0001), Some(0x42));
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn sf002_overlay_sound_pending_restore_keeps_deadline_and_refreshes_only_replayed_data() {
    for upper in [false, true] {
        for requester in [Requester::MAIN, Requester::VDP] {
            let mut cart = cartridge();
            assert!(cart.sf002.as_mut().unwrap().control(access(0xe01), 0));
            install(&mut cart, &[(0x796c, 0xa5), (0x20796c, 0xc7)]);
            let mut memory = SoundMemory::default();
            memory.set_reset(false);
            memory.bank = if upper { 0x40 } else { 0 };
            memory.ram[..4].copy_from_slice(&[0x3a, 0x6c, 0xf9, 0x76]);
            let mut cpu = Cpu::new();
            let mut audio = Audio::default();
            let mut schedule = SoundSchedule::default();
            schedule.advance(
                &mut cpu,
                &mut memory,
                &mut cart,
                &mut [],
                SoundInterval::new(0..151, &mut audio),
            );
            assert_eq!(schedule.fault, None);
            let pending = schedule.pending.as_ref().unwrap();
            assert_eq!(pending.deadline, 195);
            assert_eq!(pending.replay.is_some(), upper);
            schedule.advance(
                &mut cpu,
                &mut memory,
                &mut cart,
                &mut [],
                SoundInterval {
                    ticks: 151..1544,
                    occupied: &[Reservation {
                        requester,
                        ticks: 150..1500,
                    }],
                    audio: &mut audio,
                    sound_irq: None,
                },
            );
            assert_eq!(cpu.regs().a, 0);
            assert_eq!(schedule.pending.as_ref().unwrap().deadline, 1545);
            let prepared = schedule.clone();
            for current in [Some(0x5a), None] {
                let mut receiver_cart = cart.clone();
                install(
                    &mut receiver_cart,
                    &current.map_or_else(Vec::new, |value| vec![(0x796c, value), (0x20796c, 0x6b)]),
                );
                assert!(
                    receiver_cart
                        .sf002
                        .as_mut()
                        .unwrap()
                        .control(access(0xe01), 0x81)
                );
                assert_eq!(schedule, prepared);
                let (mut receiver_cpu, mut receiver) =
                    restore(&cpu, &schedule, &receiver_cart).unwrap();
                let mut receiver_memory = memory.clone();
                let mut receiver_audio = audio.clone();
                assert_eq!(receiver.pending.as_ref().unwrap().deadline, 1545);
                receiver.advance(
                    &mut receiver_cpu,
                    &mut receiver_memory,
                    &mut receiver_cart,
                    &mut [],
                    SoundInterval::new(1544..1545, &mut receiver_audio),
                );
                assert_eq!(receiver.fault, None);
                assert_eq!(
                    receiver_cpu.regs().a,
                    if upper { current.unwrap_or(0x17) } else { 0xa5 }
                );
                assert!(receiver.pending.is_none());
            }
        }
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn sf002_overlay_sound_consumed_prefix_change_faults_but_operand_refreshes() {
    for phase in [0, 1] {
        let mut cart = cartridge();
        assert!(cart.sf002.as_mut().unwrap().control(access(0xe01), 0));
        install(
            &mut cart,
            &[(0x207000, 0xdd), (0x207001, 0), (0x207002, 0x76)],
        );
        let mut memory = SoundMemory::default();
        memory.set_reset(false);
        memory.bank = 0x40;
        memory.ram[..3].copy_from_slice(&[0xc3, 0, 0xf0]);
        let mut cpu = Cpu::new();
        let mut audio = Audio::default();
        let mut schedule = SoundSchedule::default();
        let mut now = 0;
        reach(
            &mut cpu,
            &mut memory,
            &mut cart,
            &mut schedule,
            (&mut audio, &mut now),
            0x207000 + phase,
        );
        assert!(schedule.state.next_segment_reads_opcode());
        let (_, restored) = restore(&cpu, &schedule, &cart).unwrap();
        assert_eq!(restored, schedule);
        let mut values = vec![(0x207000, 0xdd), (0x207001, 0), (0x207002, 0x76)];
        values.push((0x207000 + phase, if phase == 0 { 0xfd } else { 1 }));
        install(&mut cart, &values);
        let before = cpu.clone();
        retire(
            &mut cpu,
            &mut memory,
            &mut cart,
            &mut audio,
            &mut schedule,
            &mut now,
        );
        assert!(matches!(
            schedule.fault,
            Some(SoundFault::UnstableCartridgeRead { .. })
        ));
        assert_eq!(cpu, before);
    }
    let mut cart = cartridge();
    assert!(cart.sf002.as_mut().unwrap().control(access(0xe01), 0));
    install(
        &mut cart,
        &[(0x207000, 0x3e), (0x207001, 0x22), (0x207002, 0x76)],
    );
    let mut memory = SoundMemory::default();
    memory.set_reset(false);
    memory.bank = 0x40;
    memory.ram[..3].copy_from_slice(&[0xc3, 0, 0xf0]);
    let mut cpu = Cpu::new();
    let mut audio = Audio::default();
    let mut schedule = SoundSchedule::default();
    let mut now = 0;
    reach(
        &mut cpu,
        &mut memory,
        &mut cart,
        &mut schedule,
        (&mut audio, &mut now),
        0x207000,
    );
    retire(
        &mut cpu,
        &mut memory,
        &mut cart,
        &mut audio,
        &mut schedule,
        &mut now,
    );
    reach(
        &mut cpu,
        &mut memory,
        &mut cart,
        &mut schedule,
        (&mut audio, &mut now),
        0x207001,
    );
    assert_eq!(schedule.pending.as_ref().unwrap().cpu.regs().a, 0x22);
    install(
        &mut cart,
        &[(0x207000, 0x3e), (0x207001, 0x33), (0x207002, 0x76)],
    );
    retire(
        &mut cpu,
        &mut memory,
        &mut cart,
        &mut audio,
        &mut schedule,
        &mut now,
    );
    assert_eq!(schedule.fault, None);
    assert_eq!(cpu.regs().a, 0x33);
}
