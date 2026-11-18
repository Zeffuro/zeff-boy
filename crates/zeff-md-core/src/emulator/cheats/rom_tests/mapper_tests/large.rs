use super::*;

mod admission;
mod consumers;
mod pending;
mod state;

fn image(size: usize, program: &[u16]) -> Vec<u8> {
    let mut bytes = extended_image(size, program);
    if size >= 0x80100c {
        write_words(
            &mut bytes,
            0x801000,
            &[0x7001, 0x0640, 3, 0x4ef9, 8, 0x1000],
        );
        bytes[0x800000..0x800003].copy_from_slice(&[0x3e, 0x11, 0x76]);
    }
    bytes
}

fn machine(program: &[u16], region: Region, concurrent: bool) -> Emulator {
    Emulator::new(image(18 * PAGE + 4, program), config(region, concurrent)).unwrap()
}

fn select(machine: &mut Emulator, slot: u32, page: u8) {
    super::select(machine, Family::Extended, slot, page);
}
