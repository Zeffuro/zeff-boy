use super::*;
use crate::hardware::audio::Audio;
use crate::hardware::bus::{Prefetch, scheduled::ScheduledBus};
use crate::hardware::sound::schedule::{SoundInterval, SoundSchedule};
use crate::hardware::sound::{CandidateBus, SoundFault, SoundMemory};
use zeff_emu_common::save_state::{StateReader, StateWriter};
use zeff_m68k::bus::{AccessKind, BusAccess, BusResponse, ByteLanes, Direction, FunctionCode};
use zeff_m68k::timing::ClockedBus;
use zeff_z80::Z80Bus;

const PC: usize = 0x50000;
const VIEW_BYTES: usize = 0x400000;

fn program(mut rom: Vec<u8>, words: &[u16]) -> Vec<u8> {
    for (index, word) in words.iter().enumerate() {
        rom[PC + 2 * index..PC + 2 * index + 2].copy_from_slice(&word.to_be_bytes());
    }
    rom
}

fn access(address: u32, lanes: ByteLanes) -> BusAccess {
    BusAccess {
        logical_address: address,
        bus_address: address & !1,
        lanes,
        function_code: FunctionCode::SupervisorData,
        kind: AccessKind::Data,
    }
}

fn enable(cart: &mut Cartridge) {
    assert_eq!(
        cart.write_download(access(0xa130f1, ByteLanes::Lower), 0),
        Some(true)
    );
}

fn natural_prefetch(rom: &[u8]) {
    for concurrent_sound in [false, true] {
        for following in [0x4e71, 0x41f8] {
            let words = [0x3039, 0x00a1, 0x3000, following, 0x0200, 0x4e71, 0x4e71];
            let config = LoadConfig {
                concurrent_sound,
                ..LoadConfig::default()
            };
            let mut machine = Emulator::new(program(rom.to_vec(), &words), config).unwrap();
            machine.step_instruction().unwrap();
            assert_eq!(machine.cpu.registers.d[0] as u16, following);
        }
    }
}

fn cpu_futures(rom: &[u8]) {
    let words = [
        0x41f9, 0x00a1, 0x3000, 0x08a8, 0, 1, 0x4e71, 0x08a8, 1, 0x00f1, 0x41f8, 0x0200, 0x70ff,
        0x20c0, 0x20c0, 0x33fc, 0x7007, 6, 0, 0x33fc, 0x4e71, 6, 2, 0x4ef9, 6, 0,
    ];
    for concurrent_sound in [false, true] {
        let config = LoadConfig {
            concurrent_sound,
            ..LoadConfig::default()
        };
        let bytes = program(rom.to_vec(), &words);
        let mut source = Emulator::new(bytes.clone(), config).unwrap();
        source.step_instruction().unwrap();
        source.step_instruction().unwrap();
        assert_eq!(source.cpu.registers.status_register() & 4, 0);
        source.step_instruction().unwrap();
        source.step_instruction().unwrap();
        assert_eq!(source.cpu.registers.status_register() & 4, 4);
        for _ in 0..4 {
            source.step_instruction().unwrap();
        }
        for address in 0x200..0x208 {
            assert_eq!(source.bus.peek(address), Some(0xff));
        }
        assert_eq!(&source.bus.cartridge.bytes[..8], &bytes[..8]);
        assert_eq!(source.bus.cartridge.bytes[0x200], 0);
        let raw = source.encode_state().unwrap();
        assert_eq!(&raw[8..12], &crate::save_state::STATE_VERSION.to_le_bytes());
        let front = source.encode_frontend_state().unwrap();
        let mut cloned = source.clone();
        let mut restored = Emulator::new(bytes.clone(), config).unwrap();
        restored.decode_state(&raw).unwrap();
        let mut frontend = Emulator::new(bytes, config).unwrap();
        frontend.decode_frontend_state(&front).unwrap();
        for current in [&mut source, &mut cloned, &mut restored, &mut frontend] {
            for _ in 0..5 {
                current.step_instruction().unwrap();
            }
            assert_eq!(current.cpu.registers.d[0], 7);
            assert_eq!(current.bus.peek(0x60000), Some(0x70));
            assert_eq!(current.bus.cartridge.bytes[0x60000], 0);
        }
        let endpoint = source.encode_state().unwrap();
        for current in [&cloned, &restored, &frontend] {
            assert_eq!(current.encode_state().unwrap(), endpoint);
        }
        let before = source.clone();
        assert!(source.reset_cpu().is_err());
        assert_eq!(source, before);
        assert!(source.warm_reset().is_err());
        assert_eq!(source, before);
        let clock = source.bus.cpu_clock;
        let cart = source.bus.cartridge.clone();
        assert_eq!(
            source.bus.begin_reset_output(clock).unwrap(),
            BusResponse::Complete(())
        );
        source.bus.end_reset_output(clock);
        assert_eq!(source.bus.cartridge, cart);
        source.reset();
        assert_eq!(source.bus.peek(0x200), Some(0));
        assert_eq!(source.bus.peek(0x300000), Some(0xff));
        assert_eq!(
            source
                .bus
                .cartridge
                .write_download(access(0x200, ByteLanes::Both), 0xffff),
            Some(true)
        );
        assert_eq!(source.bus.peek(0x200), Some(0));
    }
}

fn state_guards(rom: &[u8]) {
    let mut source = Emulator::new(rom.to_vec(), LoadConfig::default()).unwrap();
    enable(&mut source.bus.cartridge);
    assert_eq!(
        source
            .bus
            .cartridge
            .write_download(access(0x3ffffe, ByteLanes::Both), 0x1234),
        Some(true)
    );
    let raw = source.encode_state().unwrap();
    let section = raw.len() - VIEW_BYTES - 10;
    let presence = section + 4;
    assert_eq!(&raw[presence..presence + 6], &[1, 1, 0, 0, 0x40, 0]);
    let mut old = raw[..section].to_vec();
    old[8..12].copy_from_slice(&(crate::save_state::STATE_VERSION + 1).to_le_bytes());
    let mut absent = raw.clone();
    absent[presence] = 0;
    let mut size = raw.clone();
    size[presence + 2] ^= 1;
    let mut boolean = raw.clone();
    boolean[presence + 1] = 2;
    let before = source.clone();
    for bad in [&old[..], &absent, &size, &boolean, &raw[..raw.len() - 1]] {
        assert!(source.decode_state(bad).is_err());
        assert_eq!(source, before);
    }
    let mut stripped = source.clone();
    stripped.bus.cartridge.download = None;
    assert!(stripped.encode_state().is_err());
    assert!(stripped.decode_state(&raw).is_err());
    let mut plain = rom.to_vec();
    plain[0x80..0x90].fill(0);
    let plain = Emulator::new(plain, LoadConfig::default()).unwrap();
    let old = plain.encode_state().unwrap();
    assert_eq!(&old[old.len() - 5..], &[1, 0, 0, 0, 0]);
    let mut restored = plain.clone();
    restored.decode_state(&old).unwrap();
    assert_eq!(restored.encode_state().unwrap(), old);
}

fn guards(rom: &[u8]) {
    let mut machine = Emulator::new(rom.to_vec(), LoadConfig::default()).unwrap();
    let request = access(0xa13001, ByteLanes::Lower);
    for concurrent in [false, true] {
        for bad in [
            BusAccess {
                logical_address: 0x1a13001,
                ..request
            },
            BusAccess {
                function_code: FunctionCode::SupervisorProgram,
                ..request
            },
            BusAccess {
                kind: AccessKind::Stack,
                ..request
            },
        ] {
            let before = machine.clone();
            let clock = machine.bus.cpu_clock;
            let result = if concurrent {
                ScheduledBus::new(
                    &mut machine.bus,
                    &mut machine.sound_cpu,
                    &mut machine.sound_schedule,
                )
                .read_cycle(bad, clock)
            } else {
                machine.bus.read_cycle(bad, clock)
            };
            assert!(result.result.is_err());
            assert_eq!(result.wait_clocks, 0);
            assert_eq!(machine, before);
        }
    }
    for history in [
        Prefetch::Empty,
        Prefetch::UpperOnly(0x4e),
        Prefetch::Full(0x4e71),
    ] {
        machine.bus.prefetch = history;
        let clock = machine.bus.cpu_clock;
        let result = machine.bus.read_cycle(request, clock);
        assert_eq!(result.result.is_ok(), matches!(history, Prefetch::Full(_)));
        if let Ok(value) = result.result {
            assert_eq!(value, 0x71);
        }
    }
    let before = machine.bus.clone();
    let clock = machine.bus.cpu_clock;
    assert_eq!(
        machine.bus.begin_rmw(request, clock).unwrap(),
        BusResponse::Unsupported
    );
    assert_eq!(machine.bus, before);
}

fn sound(rom: &[u8]) {
    let mut cartridge = Cartridge::new(rom.to_vec()).unwrap();
    enable(&mut cartridge);
    assert_eq!(
        cartridge.write_download(access(0x300001, ByteLanes::Lower), 0x42),
        Some(true)
    );
    let mut cpu = zeff_z80::Cpu::new();
    let mut memory = SoundMemory::default();
    memory.set_reset(false);
    memory.bank = 0x60;
    memory.ram[..4].copy_from_slice(&[0x3a, 1, 0x80, 0x76]);
    let mut schedule = SoundSchedule::default();
    let mut audio = Audio::default();
    schedule.advance(
        &mut cpu,
        &mut memory,
        &mut cartridge,
        &mut [],
        SoundInterval::new(0..151, &mut audio),
    );
    assert_eq!(schedule.fault, None);
    let end = schedule.pending.as_ref().unwrap().deadline;
    schedule.validate_cartridge(&cartridge).unwrap();
    let mut writer = StateWriter::new();
    schedule.write_state(&cpu, &mut writer).unwrap();
    let mut restored = SoundSchedule::default();
    let mut restored_cpu = cpu.clone();
    restored
        .read_state(
            &mut restored_cpu,
            &mut StateReader::new(&writer.into_bytes()),
        )
        .unwrap();
    let mut restored_memory = memory.clone();
    let mut restored_audio = audio.clone();
    assert_eq!(
        cartridge.write_download(access(0x300001, ByteLanes::Lower), 0x88),
        Some(true)
    );
    for (cpu, memory, schedule, audio) in [
        (&mut cpu, &mut memory, &mut schedule, &mut audio),
        (
            &mut restored_cpu,
            &mut restored_memory,
            &mut restored,
            &mut restored_audio,
        ),
    ] {
        schedule.validate_cartridge(&cartridge).unwrap();
        schedule.advance(
            cpu,
            memory,
            &mut cartridge,
            &mut [],
            SoundInterval::new(151..end, audio),
        );
        assert_eq!(schedule.fault, None);
        assert_eq!(cpu.regs().a, 0x88);
    }
    assert_eq!(cpu, restored_cpu);
    assert_eq!(schedule, restored);
    assert_eq!(memory, restored_memory);
    assert_eq!(audio, restored_audio);
    let before = cartridge.clone();
    let mut store_memory = SoundMemory {
        bank: 0x60,
        ..SoundMemory::default()
    };
    let mut store = CandidateBus::new(&mut store_memory, &cartridge);
    store.cpu_write(0x8001, 0x5a);
    assert_eq!(store.fault.get(), None);
    assert_eq!(
        store.bank_access.get(),
        Some(crate::hardware::sound::BankAccess {
            address: 0x300001,
            direction: Direction::Write,
            value: 0x5a,
        })
    );
    assert_eq!(cartridge, before);
    for target in [0xa13001, 0xa130f1] {
        let mut memory = SoundMemory {
            bank: (target >> 15) as u16,
            ..SoundMemory::default()
        };
        let address = (0x8000 | (target & 0x7fff)) as u16;
        let mut bus = CandidateBus::new(&mut memory, &cartridge);
        bus.cpu_write(address, 0);
        assert_eq!(
            bus.fault.get(),
            Some(SoundFault::UnsupportedAccess {
                address,
                direction: Direction::Write
            })
        );
    }
}

fn movem_discard(rom: &[u8]) {
    let mut bytes = program(
        rom.to_vec(),
        &[0x4bf9, 6, 0, 0x4c9d, 0x00e0, 0x4e71, 0x4e71],
    );
    for (index, word) in [0x8000_u16, 0x3fff, 0x0100, 0xa55a].iter().enumerate() {
        bytes[0x60000 + 2 * index..0x60002 + 2 * index].copy_from_slice(&word.to_be_bytes());
    }
    for concurrent_sound in [false, true] {
        let config = LoadConfig {
            concurrent_sound,
            ..LoadConfig::default()
        };
        let mut machine = Emulator::new(bytes.clone(), config).unwrap();
        machine.step_instruction().unwrap();
        machine.step_instruction().unwrap();
        assert_eq!(
            &machine.cpu.registers.d[5..8],
            &[0xffff8000, 0x3fff, 0x0100]
        );
        assert_eq!(machine.cpu.registers.a[5], 0x60006);
        let request = BusAccess {
            kind: AccessKind::Discard,
            ..access(0x60006, ByteLanes::Both)
        };
        let before = machine.clone();
        let clock = machine.bus.cpu_clock;
        let result = if concurrent_sound {
            ScheduledBus::new(
                &mut machine.bus,
                &mut machine.sound_cpu,
                &mut machine.sound_schedule,
            )
            .write_cycle(request, 0, clock)
        } else {
            machine.bus.write_cycle(request, 0, clock)
        };
        assert!(result.result.is_err());
        assert_eq!(result.wait_clocks, 0);
        assert_eq!(machine, before);
    }
}

fn program_and_vector_reads(rom: &[u8]) {
    let mut bytes = program(rom.to_vec(), &[0x303a, 6, 0x4e4f, 0x4e71, 0x9abc, 0x4e71]);
    bytes[0xbc..0xc0].copy_from_slice(&0x60000_u32.to_be_bytes());
    bytes[0x60000..0x60006].copy_from_slice(&[0x72, 7, 0x4e, 0x71, 0x4e, 0x71]);
    for concurrent_sound in [false, true] {
        let config = LoadConfig {
            concurrent_sound,
            ..LoadConfig::default()
        };
        let mut machine = Emulator::new(bytes.clone(), config).unwrap();
        machine.step_instruction().unwrap();
        assert_eq!(machine.cpu.registers.d[0], 0x9abc);
        let sr = machine.cpu.registers.status_register();
        let sp = machine.cpu.registers.ssp;
        machine.step_instruction().unwrap();
        assert_eq!(machine.cpu.registers.pc, 0x60000);
        assert_eq!(machine.cpu.registers.ssp, sp - 6);
        let saved = [(sr >> 8) as u8, sr as u8, 0, 5, 0, 6];
        for (index, byte) in saved.into_iter().enumerate() {
            assert_eq!(machine.bus.peek(sp - 6 + index as u32), Some(byte));
        }
        machine.step_instruction().unwrap();
        assert_eq!(machine.cpu.registers.d[1], 7);
        for function_code in [FunctionCode::UserProgram, FunctionCode::SupervisorProgram] {
            let request = BusAccess {
                function_code,
                ..access(PC as u32 + 8, ByteLanes::Both)
            };
            assert_eq!(
                machine
                    .bus
                    .cartridge
                    .download_access(request, Direction::Read),
                Some(true)
            );
            assert_eq!(
                machine
                    .bus
                    .cartridge
                    .download_access(request, Direction::Write),
                Some(false)
            );
        }
        let vector = BusAccess {
            kind: AccessKind::Vector,
            ..access(0xbc, ByteLanes::Both)
        };
        assert_eq!(
            machine
                .bus
                .cartridge
                .download_access(vector, Direction::Read),
            Some(true)
        );
        assert_eq!(
            machine
                .bus
                .cartridge
                .download_access(vector, Direction::Write),
            Some(false)
        );
        for rejected in [
            BusAccess {
                function_code: FunctionCode::UserData,
                ..vector
            },
            BusAccess {
                kind: AccessKind::ResetVector,
                ..vector
            },
            BusAccess {
                logical_address: 0xa13000,
                bus_address: 0xa13000,
                ..vector
            },
        ] {
            assert_eq!(
                machine
                    .bus
                    .cartridge
                    .download_access(rejected, Direction::Read),
                Some(false)
            );
        }
    }
}

pub(crate) fn contract(rom: Vec<u8>) {
    natural_prefetch(&rom);
    cpu_futures(&rom);
    state_guards(&rom);
    guards(&rom);
    sound(&rom);
    movem_discard(&rom);
    program_and_vector_reads(&rom);
}
