use super::*;
use zeff_emu_common::save_state::{StateReader, StateWriter};
use zeff_z80::Z80Bus;

pub(super) fn assert_projection_and_replay() {
    accepted_opcode_survives_later_bank_selection();
    ignored_rom_store_keeps_valid_pending_owner();
    let mut cart = crate::cartridge::registers::bank_tests::fixture();
    for selector in [0, 1, 2, 3, 0x1f] {
        cart.registers.as_mut().unwrap().write_bank(selector);
        let mut memory = SoundMemory {
            bank: 0,
            ..SoundMemory::default()
        };
        let reader = CandidateBus::new(&mut memory, &cart);
        assert_eq!(reader.cpu_read(0x9000), 0x40 + selector);
        assert!(reader.timed_read.get());
        assert_eq!(reader.cartridge_read.get(), Some(0x9000));
        assert_eq!(reader.bank_access.get().unwrap().address, 0x1000);
    }
    for target in [0x600001, 0x600003, 0x600005, 0x700001, 0x777777, 0x7fffff] {
        let mut memory = SoundMemory {
            bank: (target >> 15) as u16,
            ..SoundMemory::default()
        };
        let before_memory = memory.clone();
        let before_cart = cart.clone();
        let mut candidate = CandidateBus::new(&mut memory, &cart);
        candidate.cpu_write((0x8000 | (target & 0x7fff)) as u16, 1);
        assert!(candidate.fault.get().is_some());
        assert_eq!(candidate.bank_access.get(), None);
        assert_eq!(candidate.register_writes, RegisterWrites::default());
        assert_eq!(memory, before_memory);
        assert_eq!(cart, before_cart);
    }
    let mut forged = RegisterWrites::default();
    assert!(forged.push(0, 1));
    assert!(forged.validate(&cart, true, None).is_err());
    cart.registers.as_mut().unwrap().write_bank(0);
    for requester in [Requester::MAIN, Requester::VDP] {
        let mut memory = SoundMemory {
            bank: 0,
            ..SoundMemory::default()
        };
        memory.set_reset(false);
        memory.ram[..4].copy_from_slice(&[0x3a, 0x00, 0x90, 0x76]);
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
        let pending = schedule.pending.as_ref().unwrap();
        assert_eq!((pending.start, pending.deadline), (150, 195));
        assert_eq!(pending.bank_access.unwrap().address, 0x1000);
        assert!(pending.replay.is_some());
        schedule.validate_cartridge(&cart).unwrap();
        for field in 0..4 {
            let mut malformed = schedule.clone();
            let replay = malformed.pending.as_mut().unwrap().replay.as_mut().unwrap();
            match field {
                0 => replay.preparation.bounded = false,
                1 => replay.accesses = 2,
                2 => replay.cycles = 6,
                _ => replay.bank = 1,
            }
            assert!(malformed.validate_cartridge(&cart).is_err());
        }
        let mut malformed = schedule.clone();
        malformed.pending.as_mut().unwrap().replay = None;
        assert!(malformed.validate_cartridge(&cart).is_err());
        let mut malformed = schedule.clone();
        malformed
            .pending
            .as_mut()
            .unwrap()
            .observed_bank_access
            .as_mut()
            .unwrap()
            .address = 0x777777;
        assert!(malformed.validate_cartridge(&cart).is_err());
        let mut writer = StateWriter::new();
        schedule.write_state(&cpu, &mut writer).unwrap();
        let encoded = writer.into_bytes();
        let mut restored = SoundSchedule::default();
        let mut other_cpu = Cpu::new();
        restored
            .read_state(&mut other_cpu, &mut StateReader::new(&encoded))
            .unwrap();
        assert_eq!(restored, schedule);
        let mut other_memory = memory.clone();
        let mut other_audio = audio.clone();
        let mut other_cart = cart.clone();
        cart.registers.as_mut().unwrap().write_bank(1);
        other_cart.registers.as_mut().unwrap().write_bank(1);
        let occupied = [Reservation {
            requester,
            ticks: 150..1500,
        }];
        for (cpu, memory, schedule, audio, cart) in [
            (&mut cpu, &mut memory, &mut schedule, &mut audio, &mut cart),
            (
                &mut other_cpu,
                &mut other_memory,
                &mut restored,
                &mut other_audio,
                &mut other_cart,
            ),
        ] {
            schedule.advance(
                cpu,
                memory,
                cart,
                &mut [],
                SoundInterval {
                    ticks: 151..1544,
                    occupied: &occupied,
                    audio,
                    sound_irq: None,
                },
            );
            assert_eq!(schedule.fault, None);
            assert_eq!(cpu.regs().a, 0);
            assert_eq!(schedule.pending.as_ref().unwrap().deadline, 1545);
            cart.registers.as_mut().unwrap().write_bank(3);
            schedule.advance(
                cpu,
                memory,
                cart,
                &mut [],
                SoundInterval::new(1544..1545, audio),
            );
            assert_eq!(schedule.fault, None);
            assert_eq!(cpu.regs().a, 0x43);
            cart.registers.as_mut().unwrap().write_bank(0);
            schedule.advance(
                cpu,
                memory,
                cart,
                &mut [],
                SoundInterval::new(1545..1605, audio),
            );
            assert_eq!(schedule.fault, None);
            assert_eq!(cpu.regs().a, 0x43);
            schedule.validate_cartridge(cart).unwrap();
        }
        assert_eq!(
            (&cpu, &memory, &schedule, &audio, &cart),
            (
                &other_cpu,
                &other_memory,
                &restored,
                &other_audio,
                &other_cart
            )
        );
    }
}

fn ignored_rom_store_keeps_valid_pending_owner() {
    let mut cart = crate::cartridge::registers::bank_tests::fixture();
    let before = cart.clone();
    let mut memory = SoundMemory {
        bank: 0,
        ..SoundMemory::default()
    };
    memory.set_reset(false);
    memory.ram[..9].copy_from_slice(&[0xdd, 0x21, 0x30, 0x80, 0xdd, 0x36, 0x3a, 0xff, 0x76]);
    let mut cpu = Cpu::new();
    let mut schedule = SoundSchedule::default();
    let mut audio = Audio::default();
    schedule.advance(
        &mut cpu,
        &mut memory,
        &mut cart,
        &mut [],
        SoundInterval::new(0..451, &mut audio),
    );
    assert_eq!(schedule.fault, None);
    let pending = schedule.pending.as_ref().unwrap();
    assert_eq!(
        pending.observed_bank_access,
        Some(BankAccess {
            address: 0x6a,
            direction: Direction::Write,
            value: 0xff
        })
    );
    assert!(pending.any_bank_access && pending.bank_access.is_none() && pending.replay.is_none());
    schedule.validate_cartridge(&cart).unwrap();
    let mut writer = StateWriter::new();
    schedule.write_state(&cpu, &mut writer).unwrap();
    let bytes = writer.into_bytes();
    let mut restored = SoundSchedule::default();
    let mut restored_cpu = Cpu::new();
    restored
        .read_state(&mut restored_cpu, &mut StateReader::new(&bytes))
        .unwrap();
    restored.validate_cartridge(&cart).unwrap();
    let mut other_memory = memory.clone();
    let mut other_cart = cart.clone();
    let mut other_audio = audio.clone();
    schedule.advance(
        &mut cpu,
        &mut memory,
        &mut cart,
        &mut [],
        SoundInterval::new(451..495, &mut audio),
    );
    restored.advance(
        &mut restored_cpu,
        &mut other_memory,
        &mut other_cart,
        &mut [],
        SoundInterval::new(451..495, &mut other_audio),
    );
    assert_eq!(schedule.fault, None);
    assert_eq!(cart, before);
    assert_eq!(
        (cpu, memory, cart, schedule, audio),
        (
            restored_cpu,
            other_memory,
            other_cart,
            restored,
            other_audio
        )
    );
}

fn accepted_opcode_survives_later_bank_selection() {
    let mut cart = crate::cartridge::registers::bank_tests::fixture();
    let bytes = std::sync::Arc::make_mut(&mut cart.bytes);
    bytes[0x1000..0x1004].copy_from_slice(&[0x3e, 0x11, 0x76, 0]);
    bytes[0x9000..0x9004].copy_from_slice(&[0x76, 0x5a, 0x76, 0]);
    let mut memory = SoundMemory {
        bank: 0,
        ..SoundMemory::default()
    };
    memory.set_reset(false);
    memory.ram[..3].copy_from_slice(&[0xc3, 0x00, 0x90]);
    let mut cpu = Cpu::new();
    let mut schedule = SoundSchedule::default();
    let mut audio = Audio::default();
    let mut now = 0;
    loop {
        schedule.advance(
            &mut cpu,
            &mut memory,
            &mut cart,
            &mut [],
            SoundInterval::new(now..now + 1, &mut audio),
        );
        now += 1;
        assert_eq!(schedule.fault, None);
        assert!(now < 1000);
        if cpu.regs().pc == 0x9001
            && schedule.state.transfer_pending()
            && !schedule.state.next_segment_reads_opcode()
        {
            break;
        }
    }
    assert_eq!(cpu.regs().a, 0);
    cart.registers.as_mut().unwrap().write_bank(1);
    assert_eq!(cart.read(0x1000), Some(0x76));
    let mut writer = StateWriter::new();
    schedule.write_state(&cpu, &mut writer).unwrap();
    let encoded = writer.into_bytes();
    let mut other_cpu = Cpu::new();
    let mut other_schedule = SoundSchedule::default();
    other_schedule
        .read_state(&mut other_cpu, &mut StateReader::new(&encoded))
        .unwrap();
    other_schedule.validate_cartridge(&cart).unwrap();
    let mut other_memory = memory.clone();
    let mut other_cart = cart.clone();
    let mut other_audio = audio.clone();
    for (cpu, memory, cart, schedule, audio) in [
        (&mut cpu, &mut memory, &mut cart, &mut schedule, &mut audio),
        (
            &mut other_cpu,
            &mut other_memory,
            &mut other_cart,
            &mut other_schedule,
            &mut other_audio,
        ),
    ] {
        schedule.advance(
            cpu,
            memory,
            cart,
            &mut [],
            SoundInterval::new(now..now + 200, audio),
        );
        assert_eq!(schedule.fault, None);
        assert_eq!(cpu.regs().a, 0x5a);
    }
    assert_eq!(
        (cpu, memory, cart, schedule, audio),
        (
            other_cpu,
            other_memory,
            other_cart,
            other_schedule,
            other_audio
        )
    );
}
