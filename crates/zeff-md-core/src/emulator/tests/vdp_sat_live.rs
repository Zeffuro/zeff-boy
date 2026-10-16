use super::*;
use crate::hardware::vdp::{ControlCommand, VdpRenderError};

const TABLE: usize = 0x2000;
const NEXT: u16 = 0x4e71;

fn command(vdp: &mut Vdp, address: u16) {
    vdp.write_control(0x4000 | address, Region::NtscUs).unwrap();
    vdp.write_control(0, Region::NtscUs).unwrap();
}

fn fixture(program: &[u16], concurrent: bool) -> Emulator {
    let mut machine = super::machine(program);
    machine.concurrent_sound = concurrent;
    machine.cpu.registers.a[0] = 0xc00000;
    machine.cpu.registers.d[0] = 0x00800000;
    let mut vdp = Vdp::bootstrap_mode5();
    vdp.advance_to(MasterTicks::new(1), Region::NtscUs).unwrap();
    vdp.write_control(0x8510, Region::NtscUs).unwrap();
    vdp.advance_to(MasterTicks::new(21), Region::NtscUs)
        .unwrap();
    vdp.write_control(0x8f02, Region::NtscUs).unwrap();
    vdp.advance_to(MasterTicks::new(56), Region::NtscUs)
        .unwrap();
    command(&mut vdp, TABLE as u16);
    machine.bus.vdp = vdp;
    machine
}

fn words(machine: &Emulator, index: u8) -> [Option<u16>; 2] {
    machine
        .vdp()
        .sprite_cache()
        .unwrap()
        .entry_words(index)
        .unwrap()
}

pub fn cpu_write_lanes_with(
    mut step: impl FnMut(&mut Emulator) -> Result<MachineStep, MachineFault>,
) {
    for concurrent in [false, true] {
        for (opcode, value, address, expected) in [
            (0x33fc, 0x1280, 0xc00002, 0x0280),
            (0x13fc, 0x00a1, 0xc00000, 0x01a1),
            (0x13fc, 0x00a1, 0xc00001, 0x01a1),
        ] {
            let mut machine = fixture(
                &[
                    opcode,
                    value,
                    (address >> 16) as u16,
                    address as u16,
                    NEXT,
                    NEXT,
                ],
                concurrent,
            );
            assert_eq!(words(&machine, 0), [None, None]);
            let mut copied = machine.clone();
            let result = step(&mut machine).unwrap();
            assert_eq!(step(&mut copied), Ok(result));
            assert_eq!(machine, copied);
            assert_eq!(words(&machine, 0), [Some(expected), None]);
            assert!(machine.vdp().fifo().is_empty());
        }
    }
}

pub fn fifo_waits_and_sprite_lookup_with(
    mut step: impl FnMut(&mut Emulator) -> Result<MachineStep, MachineFault>,
) {
    for concurrent in [false, true] {
        let mut machine = fixture(&[0x2080, NEXT, NEXT], concurrent);
        for word in [0x0080, 0, 1, 0x0080] {
            machine.bus.vdp.write_data(word, Region::NtscUs).unwrap();
        }
        machine.cpu.registers.d[0] = 0x00a50b00;
        assert_eq!(
            machine.vdp().sprite_cache(),
            Err(VdpRenderError::PendingSpriteWrites)
        );
        let result = step(&mut machine).unwrap();
        assert_eq!(result.elapsed.get(), 15 * MAIN_CPU_DIVISOR);
        assert_eq!(machine.bus.now.get(), 161);
        assert_eq!(
            machine.vdp().sprite_cache(),
            Err(VdpRenderError::PendingSpriteWrites)
        );
        machine.bus.advance_machine(200).unwrap();
        assert_eq!(words(&machine, 0), [Some(0x80), Some(0)]);
        assert_eq!(words(&machine, 1), [Some(0xa5), Some(0xb00)]);
        let cache = machine.vdp().sprite_cache().unwrap();
        let sprite = cache
            .sprite_entry(machine.vdp().memory(), machine.vdp().registers(), 0)
            .unwrap();
        assert_eq!(
            (sprite.x, sprite.y, sprite.width, sprite.height, sprite.name),
            (0, 0, 8, 8, 1)
        );
        let row = cache.select_row(0).unwrap();
        assert_eq!(row.indices(), &[0]);
        let pattern = row
            .prepare_sprite(cache, machine.vdp().memory(), machine.vdp().registers(), 0)
            .unwrap()
            .unwrap();
        assert_eq!(pattern.row_address(0), Some(32));
    }
}

pub fn guest_table_publication_with(
    mut step: impl FnMut(&mut Emulator) -> Result<MachineStep, MachineFault>,
) {
    for concurrent in [false, true] {
        let mut machine = fixture(&[0x33fc, 0x8511, 0x00c0, 4, NEXT, NEXT], concurrent);
        for word in [0x0080, 0, 1, 0x0080] {
            machine.bus.vdp.write_data(word, Region::NtscUs).unwrap();
        }
        step(&mut machine).unwrap();
        assert_eq!(
            machine.vdp().sprite_cache(),
            Err(VdpRenderError::PendingSpriteWrites)
        );
        assert_eq!(&machine.vdp().memory().vram()[TABLE..TABLE + 2], &[0, 0x80]);
        step(&mut machine).unwrap();
        machine.bus.advance_machine(240).unwrap();
        assert_eq!(machine.vdp().registers().registers()[5], Some(0x11));
        assert_eq!(words(&machine, 0), [Some(0x80), Some(0)]);
        assert_eq!(
            &machine.vdp().memory().vram()[TABLE..TABLE + 8],
            &[0, 0x80, 0, 0, 0, 1, 0, 0x80]
        );
        command(&mut machine.bus.vdp, TABLE as u16 + 512);
        machine.bus.vdp.write_data(0x00a5, Region::NtscUs).unwrap();
        machine.bus.advance_machine(300).unwrap();
        assert_eq!(words(&machine, 0), [Some(0xa5), Some(0)]);
    }
}

#[test]
fn register_then_long_data_write_discards_both_words() {
    for concurrent in [false, true] {
        let mut machine = fixture(
            &[
                0x33fc, 0x8511, 0x00c0, 4, 0x23fc, 0x1234, 0x5678, 0x00c0, 0, NEXT, NEXT,
            ],
            concurrent,
        );
        for word in [0x0080, 0, 1, 0x0080] {
            machine.bus.vdp.write_data(word, Region::NtscUs).unwrap();
        }
        machine.step_instruction().unwrap();
        machine.step_instruction().unwrap();
        machine.bus.advance_machine(500).unwrap();
        assert_eq!(
            machine.vdp().command(),
            ControlCommand {
                address: 0x515,
                code: 0x12
            }
        );
        assert_eq!(&machine.vdp().memory().vram()[0x510..0x518], &[0; 8]);
        assert_eq!(
            &machine.vdp().memory().vram()[TABLE..TABLE + 8],
            &[0, 0x80, 0, 0, 0, 1, 0, 0x80]
        );
        assert_eq!(words(&machine, 0), [Some(0x80), Some(0)]);
    }
}

pub fn reset_domains_with(
    mut restart: impl FnMut(&mut Emulator) -> Result<(), TimingFault<BusError>>,
    mut power_on: impl FnMut(&mut Emulator),
) {
    for concurrent in [false, true] {
        let mut machine = fixture(&[NEXT; 3], concurrent);
        machine.bus.vdp.write_data(0x0080, Region::NtscUs).unwrap();
        machine.bus.vdp.write_data(0x0b00, Region::NtscUs).unwrap();
        machine.bus.advance_machine(100).unwrap();
        let pointer = machine.vdp().memory().vram().as_ptr();
        assert_eq!(words(&machine, 0), [Some(0x80), Some(0xb00)]);
        restart(&mut machine).unwrap();
        assert_eq!(words(&machine, 0), [Some(0x80), Some(0xb00)]);
        assert_eq!(machine.vdp().memory().vram().as_ptr(), pointer);
        power_on(&mut machine);
        for index in 0..64 {
            assert_eq!(words(&machine, index), [None, None]);
        }
        assert_eq!(machine.vdp().memory().vram().as_ptr(), pointer);
    }
}

pub fn failed_prime_restores_cache_with(
    mut restart: impl FnMut(&mut Emulator) -> Result<(), TimingFault<BusError>>,
) {
    for concurrent in [false, true] {
        let mut machine = fixture(&[NEXT; 3], concurrent);
        machine.bus.vdp.write_data(0x0080, Region::NtscUs).unwrap();
        machine.bus.vdp.write_data(0x0b00, Region::NtscUs).unwrap();
        machine
            .bus
            .vdp
            .write_control(0x4567, Region::NtscUs)
            .unwrap();
        machine.bus.cartridge.initial.pc = 0xc0000e;
        let pointer = machine.vdp().memory().vram().as_ptr();
        let saved = machine.clone();
        assert!(matches!(
            restart(&mut machine),
            Err(TimingFault::Bus {
                access: zeff_m68k::bus::BusAccess {
                    bus_address: 0xc00010,
                    ..
                },
                source: BusError::UnsupportedAccess {
                    address: 0xc00010,
                    direction: zeff_m68k::bus::Direction::Read
                },
                ..
            })
        ));
        assert_eq!(machine, saved);
        assert_eq!(machine.vdp().control().pending_word(), Some(0x4567));
        assert_eq!(machine.vdp().memory().vram().as_ptr(), pointer);
        assert_eq!(&machine.vdp().memory().vram()[TABLE..TABLE + 4], &[0; 4]);
        machine.bus.advance_machine(100).unwrap();
        assert_eq!(words(&machine, 0), [Some(0x80), Some(0xb00)]);
    }
}

pub fn reset_output_preserves_cache_with(
    mut step: impl FnMut(&mut Emulator) -> Result<MachineStep, MachineFault>,
) {
    for concurrent in [false, true] {
        let mut machine = fixture(&[0x4e70, NEXT, NEXT], concurrent);
        machine.bus.vdp.write_data(0x0080, Region::NtscUs).unwrap();
        machine.bus.vdp.write_data(0x0b00, Region::NtscUs).unwrap();
        machine.bus.advance_machine(100).unwrap();
        let cache = *machine.vdp().sprite_cache().unwrap();
        step(&mut machine).unwrap();
        assert_eq!(machine.reset_pulses(), 1);
        assert_eq!(machine.vdp().sprite_cache().unwrap(), &cache);
    }
}

#[test]
fn cpu_write_lanes() {
    cpu_write_lanes_with(Emulator::step_instruction);
}
#[test]
fn fifo_waits_and_sprite_lookup() {
    fifo_waits_and_sprite_lookup_with(Emulator::step_instruction);
}
#[test]
fn guest_table_publication() {
    guest_table_publication_with(Emulator::step_instruction);
}
#[test]
fn reset_domains() {
    reset_domains_with(Emulator::reset_cpu, Reset::reset);
}
#[test]
fn failed_prime_restores_cache() {
    failed_prime_restores_cache_with(Emulator::reset_cpu);
}
#[test]
fn reset_output_preserves_cache() {
    reset_output_preserves_cache_with(Emulator::step_instruction);
}
