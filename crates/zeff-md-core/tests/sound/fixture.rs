use zeff_m68k::timing::TimedOutcome;
use zeff_md_core::{Emulator, LoadConfig};

pub const SOUND_RAM: u32 = 0xa0_0000;
pub const BUS_REQUEST: u32 = 0xa1_1100;
pub const RESET: u32 = 0xa1_1200;
pub const IRQ_HANDLER: usize = 0x600;

pub fn store_byte(program: &mut Vec<u16>, address: u32, value: u8) {
    program.extend_from_slice(&[
        0x13fc,
        u16::from(value),
        (address >> 16) as u16,
        address as u16,
    ]);
}

pub fn store_word(program: &mut Vec<u16>, address: u32, value: u16) {
    program.extend_from_slice(&[0x33fc, value, (address >> 16) as u16, address as u16]);
}

pub fn image(program: &[u16], handler: &[u16]) -> Vec<u8> {
    let size = 0x1000
        .max(0x100 + program.len() * 2)
        .max(IRQ_HANDLER + handler.len() * 2);
    let mut image = vec![0; size];
    image[..8].copy_from_slice(&[0, 255, 255, 252, 0, 0, 1, 0]);
    image[112..116].copy_from_slice(&(IRQ_HANDLER as u32).to_be_bytes());
    for (base, words) in [(0x100, program), (IRQ_HANDLER, handler)] {
        for (index, word) in words.iter().enumerate() {
            image[base + 2 * index..base + 2 * index + 2].copy_from_slice(&word.to_be_bytes());
        }
    }
    image
}

pub fn new(program: &[u16], handler: &[u16]) -> Emulator {
    Emulator::new(image(program, handler), LoadConfig::default()).unwrap()
}

pub fn load_program(sound: &[u8]) -> Vec<u16> {
    let mut program = Vec::new();
    store_word(&mut program, BUS_REQUEST, 0x0100);
    store_word(&mut program, RESET, 0x0100);
    for (index, byte) in sound.iter().copied().enumerate() {
        store_byte(&mut program, SOUND_RAM + index as u32, byte);
    }
    program
}

pub fn stopped(sound: &[u8], handler: &[u16]) -> Emulator {
    stopped_with_bank(sound, handler, 0)
}

pub fn stopped_with_bank(sound: &[u8], handler: &[u16], bank: u16) -> Emulator {
    let mut program = load_program(sound);
    if bank != 0 {
        for bit in 0..9 {
            store_byte(&mut program, SOUND_RAM + 0x6000, ((bank >> bit) & 1) as u8);
        }
    }
    store_word(&mut program, BUS_REQUEST, 0);
    program.extend_from_slice(&[0x4e72, 0x2000, 0x4e71, 0x4e71]);
    let mut machine = new(&program, handler);
    for _ in 0..sound.len() + 4 + if bank == 0 { 0 } else { 9 } {
        instruction(&mut machine);
    }
    assert_eq!(
        machine.step_instruction().unwrap().cpu,
        TimedOutcome::Stopped
    );
    machine
}

pub fn instruction(machine: &mut Emulator) {
    assert!(matches!(
        machine.step_instruction().unwrap().cpu,
        TimedOutcome::Instruction(_)
    ));
}

pub fn wake(machine: &mut Emulator) {
    machine.set_interrupt_level(4);
    assert!(matches!(
        machine.step_instruction().unwrap().cpu,
        TimedOutcome::Interrupt { level: 4, .. }
    ));
    machine.set_interrupt_level(0);
}
