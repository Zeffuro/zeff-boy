use super::*;
use zeff_emu_common::save_state::{StateReader, StateWriter};
use zeff_z80::Z80Bus;

fn mapping(cart: &mut Cartridge, bank: u8, expanded: bool) {
    let board = cart.sf004.as_mut().unwrap();
    board.power_on();
    assert!(board.write(0xf01, bank << 4));
    assert!(board.write(0xe01, if expanded { 0xc0 } else { 0x80 }));
}

fn fixture(cartridge: &Cartridge) -> Cartridge {
    let mut cart = cartridge.clone();
    let bytes = std::sync::Arc::make_mut(&mut cart.bytes);
    for bank in 0..8 {
        bytes[(bank << 18) + 0x1234] = 0x30 + bank as u8;
    }
    mapping(&mut cart, 0, false);
    cart
}

fn memory(program: &[u8], target: u32) -> SoundMemory {
    let mut memory = SoundMemory::default();
    memory.set_reset(false);
    memory.bank = (target >> SOUND_BANK_SHIFT) as u16;
    memory.ram[..program.len()].copy_from_slice(program);
    memory
}

fn pending_read(cart: &mut Cartridge, target: u32) -> (Cpu, SoundMemory, SoundSchedule, Audio) {
    let [lo, hi] = (0x8000 | (target & SOUND_MAIN_MASK) as u16).to_le_bytes();
    let mut memory = memory(&[0x3a, lo, hi, 0x76], target);
    let mut cpu = Cpu::new();
    let mut schedule = SoundSchedule::default();
    let mut audio = Audio::default();
    schedule.advance(
        &mut cpu,
        &mut memory,
        cart,
        &mut [],
        SoundInterval::new(0..151, &mut audio),
    );
    assert_eq!(schedule.fault, None);
    let pending = schedule.pending.as_ref().unwrap();
    assert_eq!(pending.bank_access.unwrap().address, target);
    assert_eq!(pending.bank_access, pending.observed_bank_access);
    assert!(pending.replay.is_some());
    assert_eq!(pending.reservation().unwrap().requester, Requester::SOUND);
    assert_eq!(cpu.regs().a, 0);
    schedule.validate_cartridge(cart).unwrap();
    (cpu, memory, schedule, audio)
}

fn candidate_boundaries(cartridge: &Cartridge) {
    let mut memory = memory(&[], 0);
    for direction in [Direction::Read, Direction::Write] {
        for bank in [0, 0x40, 0x142, 0x1c0] {
            memory.bank = bank;
            let before = memory.clone();
            let mut candidate = CandidateBus::new(&mut memory, cartridge);
            let permitted = direction == Direction::Read && bank == 0;
            match direction {
                Direction::Read => {
                    let value = candidate.cpu_read(0x8001);
                    if permitted {
                        assert_eq!(Some(value), cartridge.read(1));
                        assert!(candidate.timed_read.get());
                        assert_eq!(candidate.cartridge_read.get(), Some(0x8001));
                        assert_eq!(candidate.bank_access.get().unwrap().address, 1);
                    }
                }
                Direction::Write => candidate.cpu_write(0x8001, 0x81),
            }
            assert_eq!(candidate.fault.get().is_none(), permitted);
            assert!(candidate.sf001_writes.is_empty());
            assert!(candidate.sf002_writes.is_empty());
            assert!(candidate.writes.0.iter().all(Option::is_none));
            assert_eq!(*candidate.memory, before);
        }
    }
    let mut candidate = CandidateBus::new(&mut memory, cartridge);
    candidate.cpu_write(0x0010, 0x5a);
    assert_eq!(candidate.cpu_read(0x0010), 0x5a);
    assert_eq!(candidate.fault.get(), None);
    let cpu = Cpu::new();
    let schedule = SoundSchedule {
        pending: Some(PendingSound::prepare(&cpu, &memory, cartridge, 0).unwrap()),
        ..Default::default()
    };
    schedule.validate_cartridge(cartridge).unwrap();
    for which in 0..4 {
        let mut bad = schedule.clone();
        let pending = bad.pending.as_mut().unwrap();
        let access = BankAccess {
            address: 0,
            direction: Direction::Read,
            value: 0,
        };
        match which {
            0 => pending.any_bank_access = true,
            1 => pending.bank_access = Some(access),
            2 => pending.observed_bank_access = Some(access),
            _ => {
                pending.replay = Some(ReadReplay {
                    preparation: Preparation {
                        now: 0,
                        state: BusState::default(),
                        bounded: false,
                        opn2_status: 0,
                    },
                    bank: 0,
                    irq_asserted: false,
                    cycles: 1,
                    accesses: 1,
                    bank_identity: Some((0, Direction::Read)),
                })
            }
        }
        assert!(bad.validate_cartridge(cartridge).is_err());
    }
}

fn malformed_pending(schedule: &SoundSchedule, cartridge: &Cartridge) {
    for which in 0..12 {
        let mut bad = schedule.clone();
        let pending = bad.pending.as_mut().unwrap();
        match which {
            0 => pending.any_bank_access = false,
            1 => pending.observed_bank_access = None,
            2 => pending.bank_access = None,
            3 => pending.replay = None,
            4 => pending.observed_bank_access.as_mut().unwrap().direction = Direction::Write,
            5 => pending.bank_access.as_mut().unwrap().address ^= 1,
            6 => pending.replay.as_mut().unwrap().bank_identity = Some((0x200001, Direction::Read)),
            7 => pending.replay.as_mut().unwrap().bank ^= 1,
            8 => pending.memory.bank ^= 1,
            9 => pending.replay.as_mut().unwrap().accesses = 2,
            10 => pending.replay.as_mut().unwrap().cycles = 2,
            _ => pending.replay.as_mut().unwrap().preparation.bounded = false,
        }
        assert!(bad.validate_cartridge(cartridge).is_err());
    }
    for (address, direction) in [
        (0x200001, Direction::Read),
        (0xa13001, Direction::Read),
        (0xe00001, Direction::Read),
        (0x41234, Direction::Write),
    ] {
        let mut bad = schedule.clone();
        let pending = bad.pending.as_mut().unwrap();
        let access = BankAccess {
            address,
            direction,
            value: 0,
        };
        pending.bank_access = Some(access);
        pending.observed_bank_access = Some(access);
        pending.memory.bank = (address >> SOUND_BANK_SHIFT) as u16;
        let replay = pending.replay.as_mut().unwrap();
        replay.bank = pending.memory.bank;
        replay.bank_identity = Some((address, direction));
        assert!(bad.validate_cartridge(cartridge).is_err());
    }
}

fn publication_and_restore(cartridge: &Cartridge) {
    for requester in [Requester::MAIN, Requester::VDP] {
        for (bank, expanded, expected) in [(2, false, 0x32), (0, true, 0x31), (2, true, 0x33)] {
            let mut cart = fixture(cartridge);
            let (mut cpu, mut memory, mut schedule, mut audio) = pending_read(&mut cart, 0x41234);
            let occupied = [Reservation {
                requester,
                ticks: 150..1500,
            }];
            schedule.advance(
                &mut cpu,
                &mut memory,
                &mut cart,
                &mut [],
                SoundInterval {
                    ticks: 151..1544,
                    occupied: &occupied,
                    audio: &mut audio,
                    sound_irq: None,
                },
            );
            assert_eq!(schedule.fault, None);
            assert_eq!(cpu.regs().a, 0);
            assert_eq!(schedule.pending.as_ref().unwrap().deadline, 1545);
            mapping(&mut cart, bank, expanded);
            memory.bank = 0x1ff;
            memory.ram[0x777] = 0xa6;
            malformed_pending(&schedule, &cart);
            let mut writer = StateWriter::new();
            schedule.write_state(&cpu, &mut writer).unwrap();
            let bytes = writer.into_bytes();
            let mut restored_cpu = Cpu::new();
            let mut restored = SoundSchedule::default();
            let mut reader = StateReader::new(&bytes);
            restored.read_state(&mut restored_cpu, &mut reader).unwrap();
            assert!(reader.is_exhausted());
            restored.validate_cartridge(&cart).unwrap();
            assert_eq!(schedule, restored);
            assert_eq!(cpu, restored_cpu);
            let mut other_memory = memory.clone();
            let mut other_cart = cart.clone();
            let mut other_audio = audio.clone();
            for (cpu, memory, cart, schedule, audio) in [
                (&mut cpu, &mut memory, &mut cart, &mut schedule, &mut audio),
                (
                    &mut restored_cpu,
                    &mut other_memory,
                    &mut other_cart,
                    &mut restored,
                    &mut other_audio,
                ),
            ] {
                let before = cart.clone();
                schedule.advance(
                    cpu,
                    memory,
                    cart,
                    &mut [],
                    SoundInterval::new(1544..1545, audio),
                );
                assert_eq!(schedule.fault, None);
                assert_eq!(cpu.regs().a, expected);
                assert_eq!(memory.bank, 8);
                assert_eq!(memory.ram[0x777], 0xa6);
                assert!(schedule.pending.is_none());
                assert_eq!(*cart, before);
            }
            assert_eq!(
                (cpu, memory, cart, audio, schedule),
                (
                    restored_cpu,
                    other_memory,
                    other_cart,
                    other_audio,
                    restored
                )
            );
        }
    }
}

fn fetch_and_holes(cartridge: &Cartridge) {
    for changed in [false, true] {
        let mut cart = fixture(cartridge);
        let bytes = std::sync::Arc::make_mut(&mut cart.bytes);
        bytes[0x1234] = 0;
        bytes[0x41234] = if changed { 0x76 } else { 0 };
        let mut memory = memory(&[0xc3, 0x34, 0x92], 0);
        let mut cpu = Cpu::new();
        let mut schedule = SoundSchedule::default();
        let mut audio = Audio::default();
        schedule.advance(
            &mut cpu,
            &mut memory,
            &mut cart,
            &mut [],
            SoundInterval::new(0..151, &mut audio),
        );
        assert_eq!(schedule.fault, None);
        assert!(schedule.state.next_segment_reads_opcode());
        schedule.validate_cartridge(&cart).unwrap();
        let deadline = schedule.pending.as_ref().unwrap().deadline;
        let before = cpu.clone();
        mapping(&mut cart, 1, false);
        schedule.advance(
            &mut cpu,
            &mut memory,
            &mut cart,
            &mut [],
            SoundInterval::new(151..deadline, &mut audio),
        );
        if changed {
            assert!(matches!(
                schedule.fault,
                Some(SoundFault::UnstableCartridgeRead { .. })
            ));
            assert_eq!(cpu, before);
        } else {
            assert_eq!(schedule.fault, None);
            assert_ne!(cpu, before);
        }
    }
    let mut cart = fixture(cartridge);
    let (mut cpu, mut memory, mut schedule, mut audio) = pending_read(&mut cart, 0x181234);
    let before_cpu = cpu.clone();
    let before_memory = memory.clone();
    let deadline = schedule.pending.as_ref().unwrap().deadline;
    mapping(&mut cart, 0, true);
    assert!(schedule.validate_cartridge(&cart).is_err());
    let before_cart = cart.clone();
    schedule.advance(
        &mut cpu,
        &mut memory,
        &mut cart,
        &mut [],
        SoundInterval::new(151..deadline, &mut audio),
    );
    assert!(matches!(
        schedule.fault,
        Some(SoundFault::UnsupportedAccess {
            direction: Direction::Read,
            ..
        })
    ));
    assert_eq!(cpu, before_cpu);
    assert_eq!(memory, before_memory);
    assert_eq!(cart, before_cart);
}

impl SoundSchedule {
    pub(crate) fn assert_sf004_boundaries(cartridge: &Cartridge) {
        candidate_boundaries(cartridge);
        publication_and_restore(cartridge);
        fetch_and_holes(cartridge);
    }
}
