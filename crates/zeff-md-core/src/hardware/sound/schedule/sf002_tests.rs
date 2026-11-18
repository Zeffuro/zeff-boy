use super::*;

mod overlay;
use crate::cartridge::sf002::Mapping;
use crate::emulator::sf002_tests::{access, cartridge};
use crate::hardware::bus::{Bus, scheduled::ScheduledBus};
use zeff_emu_common::save_state::{StateReader, StateWriter};
use zeff_m68k::timing::{ClockedBus, CpuClocks};
use zeff_z80::Z80Bus;

fn restore(
    cpu: &Cpu,
    schedule: &SoundSchedule,
    cartridge: &Cartridge,
) -> anyhow::Result<(Cpu, SoundSchedule)> {
    let mut writer = StateWriter::new();
    schedule.write_state(cpu, &mut writer)?;
    let bytes = writer.into_bytes();
    let mut other_cpu = Cpu::new();
    let mut other = SoundSchedule::default();
    let mut reader = StateReader::new(&bytes);
    other.read_state(&mut other_cpu, &mut reader)?;
    assert!(reader.is_exhausted());
    other.validate_cartridge(cartridge)?;
    assert_eq!((cpu, schedule), (&other_cpu, &other));
    Ok((other_cpu, other))
}
#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn sf002_ordered_sound_journal_pending_replay_and_raw27_schedule() {
    let mut cart = cartridge();
    let before = cart.clone();
    let mut memory = SoundMemory::default();
    let mut candidate = CandidateBus::new(&mut memory, &cart);
    candidate.cpu_write(0x8e01, 0);
    candidate.cpu_write(0x8e01, 0x81);
    candidate.memory.bank = 0x40;
    assert_eq!(candidate.cpu_read(0xf96c), 0x17);
    assert_eq!(candidate.fault.get(), None);
    let journal = candidate.sf002_writes;
    assert_eq!(cart, before);
    journal.project(&mut cart).unwrap();
    assert_eq!(cart.sf002.as_ref().unwrap().mapping(), Mapping::Mirrored);
    let mut memory = SoundMemory {
        bank: 0x78,
        ..Default::default()
    };
    let mut candidate = CandidateBus::new(&mut memory, &cart);
    candidate.cpu_write(0x8001, 0x42);
    assert_eq!(candidate.cpu_read(0x8001), 0x42);
    assert_eq!(candidate.fault.get(), None);
    let save_journal = candidate.sf002_writes;
    assert_eq!(cart.read(0x3c0001), Some(0xff));
    save_journal.publish(&mut cart);
    assert_eq!(cart.read(0x3c0001), Some(0x42));
    for target in [0xe01, 0xe00, 0xa130f1, 0x3c4001] {
        let mut fresh = cartridge();
        let original = fresh.clone();
        let mut memory = SoundMemory {
            bank: (target >> SOUND_BANK_SHIFT) as u16,
            ..Default::default()
        };
        let mut candidate = CandidateBus::new(&mut memory, &fresh);
        candidate.cpu_write(0x8000 | (target & SOUND_MAIN_MASK) as u16, 0x81);
        assert!(candidate.fault.get().is_some());
        assert!(candidate.sf002_writes.is_empty());
        assert_eq!(fresh, original);
        fresh.sf002.as_mut().unwrap().control(access(0xe01), 0);
    }
    {
        let mut cart = cartridge();
        let mut memory = SoundMemory::default();
        memory.set_reset(false);
        memory.ram[..4].copy_from_slice(&[0x32, 1, 0x8e, 0x76]);
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
        assert_eq!(
            pending.bank_access,
            Some(BankAccess {
                address: 0xe01,
                direction: Direction::Write,
                value: 0
            })
        );
        let (mut other_cpu, mut other) = restore(&cpu, &schedule, &cart).unwrap();
        assert_eq!((&cpu, &schedule), (&other_cpu, &other));
        let mut bus = Bus::new(cart.clone(), crate::Region::NtscUs);
        bus.advance_machine(151).unwrap();
        bus.sound = memory.clone();
        let before = (bus.clone(), other_cpu.clone(), other.clone());
        assert!(
            ScheduledBus::new(&mut bus, &mut other_cpu, &mut other)
                .write_cycle(access(0xe01), 0x81, CpuClocks(1))
                .result
                .is_err()
        );
        assert_eq!((bus.clone(), other_cpu.clone(), other.clone()), before);
        let clock = bus.cpu_clock;
        ScheduledBus::new(&mut bus, &mut other_cpu, &mut other)
            .write_cycle(access(0xe01), 0x81, clock)
            .result
            .unwrap();
        assert_eq!(other.fault, None);
        assert_eq!(
            bus.cartridge.sf002.as_ref().unwrap().mapping(),
            Mapping::Mirrored
        );
        for second in [(0xe01, 0x81), (0x3c0001, 0x42)] {
            let mut forged = schedule.clone();
            let mut writes = pending.sf002_writes;
            assert!(writes.push(&cart, second.0, second.1));
            forged.pending.as_mut().unwrap().sf002_writes = writes;
            let before = (
                cpu.clone(),
                schedule.clone(),
                cart.clone(),
                memory.clone(),
                audio.clone(),
            );
            let error = restore(&cpu, &forged, &cart).unwrap_err();
            assert!(
                error
                    .to_string()
                    .contains("multiple SF-002 writes in one pending transfer")
            );
            assert_eq!(
                (
                    cpu.clone(),
                    schedule.clone(),
                    cart.clone(),
                    memory.clone(),
                    audio.clone()
                ),
                before
            );
        }
        schedule.advance(
            &mut cpu,
            &mut memory,
            &mut cart,
            &mut [],
            SoundInterval::new(151..195, &mut audio),
        );
        assert_eq!(schedule.fault, None);
        assert_eq!(cart.sf002.as_ref().unwrap().mapping(), Mapping::Linear);
        assert_eq!(cart.read(0x3c0001), Some(0xff));
    }
    for requester in [Requester::MAIN, Requester::VDP] {
        let mut cart = cartridge();
        cart.sf002.as_mut().unwrap().control(access(0xe01), 0);
        let mut cpu = Cpu::new();
        let mut memory = SoundMemory::default();
        memory.set_reset(false);
        memory.bank = 0x40;
        memory.ram[..4].copy_from_slice(&[0x3a, 0x6c, 0xf9, 0x76]);
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
        assert!(schedule.pending.as_ref().unwrap().replay.is_some());
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
        cart.sf002.as_mut().unwrap().control(access(0xe01), 0x81);
        let mut writer = StateWriter::new();
        schedule.write_state(&cpu, &mut writer).unwrap();
        let bytes = writer.into_bytes();
        let mut other_cpu = Cpu::new();
        let mut other = SoundSchedule::default();
        other
            .read_state(&mut other_cpu, &mut StateReader::new(&bytes))
            .unwrap();
        other.validate_cartridge(&cart).unwrap();
        assert_eq!((&cpu, &schedule), (&other_cpu, &other));
        for address in [0x3c4000, 0x3c4001, 0x3fffff] {
            let mut forged = other.clone();
            let pending = forged.pending.as_mut().unwrap();
            pending.memory.bank = (address >> SOUND_BANK_SHIFT) as u16;
            pending.bank_access.as_mut().unwrap().address = address;
            pending.observed_bank_access.as_mut().unwrap().address = address;
            pending.replay = None;
            let before = (other_cpu.clone(), other.clone(), cart.clone());
            let error = restore(&other_cpu, &forged, &cart).unwrap_err();
            assert!(
                error
                    .to_string()
                    .contains("unsupported SF-002 pending read")
            );
            assert_eq!((other_cpu.clone(), other.clone(), cart.clone()), before);
        }
        let mut other_cart = cart.clone();
        let mut other_memory = memory.clone();
        let mut other_audio = audio.clone();
        schedule.advance(
            &mut cpu,
            &mut memory,
            &mut cart,
            &mut [],
            SoundInterval::new(1544..1545, &mut audio),
        );
        other.advance(
            &mut other_cpu,
            &mut other_memory,
            &mut other_cart,
            &mut [],
            SoundInterval::new(1544..1545, &mut other_audio),
        );
        assert_eq!(schedule.fault, None);
        assert_eq!(cpu.regs().a, 0x17);
        assert_eq!(
            (cpu, memory, cart, audio, schedule),
            (other_cpu, other_memory, other_cart, other_audio, other)
        );
    }
}
