use super::*;
use crate::hardware::bus::scheduled::ScheduledBus;
use zeff_m68k::bus::Direction;

mod eeprom;
mod owners;
mod sound;
mod state;

const REGIONS: [Region; 3] = [Region::NtscUs, Region::NtscJapan, Region::Pal];
const PORT: u32 = 0x200001;
const BANK_SOURCE: u32 = 0x181000;

#[derive(Clone, Copy, Debug)]
enum Profile {
    Odd512,
    Odd8,
    Odd32,
    Parallel,
    BoundedParallel,
    Serial128,
    Serial256,
    Split256,
    Split2048,
    Split8192,
    Switched8,
    Switched32,
    Time8,
    Sf004,
}

const NATURAL: [Profile; 14] = [
    Profile::Odd512,
    Profile::Odd8,
    Profile::Odd32,
    Profile::Parallel,
    Profile::BoundedParallel,
    Profile::Serial128,
    Profile::Serial256,
    Profile::Split256,
    Profile::Split2048,
    Profile::Split8192,
    Profile::Switched8,
    Profile::Switched32,
    Profile::Time8,
    Profile::Sf004,
];

fn config(region: Region, concurrent_sound: bool) -> LoadConfig {
    LoadConfig {
        region,
        concurrent_sound,
    }
}

fn save_image(profile: Profile) -> Vec<u8> {
    let (size, product, declaration): (usize, &[u8; 14], &[u8; 12]) = match profile {
        Profile::Odd512 => (
            0x100000,
            b"SYNTHETIC     ",
            b"RA\xf8\x20\x00\x20\x00\x01\x00\x20\x03\xff",
        ),
        Profile::Odd8 => (
            0x100000,
            b"SYNTHETIC     ",
            b"RA\xf8\x20\x00\x20\x00\x01\x00\x20\x3f\xff",
        ),
        Profile::Odd32 => (
            0x100000,
            b"SYNTHETIC     ",
            b"RA\xf8\x20\x00\x20\x00\x01\x00\x20\xff\xff",
        ),
        Profile::Parallel => (
            0x200000,
            b"GM T-119186-00",
            b"RA\xf8\x40\x00\x20\x00\x01\x00\x20\x0f\xff",
        ),
        Profile::BoundedParallel => (
            0x200000,
            b"GM T-119156-00",
            b"RA\xf8\x40\x00\x20\x00\x01\x00\x20\x07\xff",
        ),
        Profile::Serial128 => (
            0x10000,
            b"GM T-12046 -00",
            b"RA\xe8\x40\x00\x20\x00\x01\x00\x20\x00\x01",
        ),
        Profile::Serial256 => (
            0x200000,
            b"GM T-81033  00",
            b"RA\xe8\x40\x00\x20\x00\x01\x00\x20\x00\x01",
        ),
        Profile::Split256 => (
            0x300000,
            b"GM T-081276 00",
            b"RA\xe8\x40\x00\x20\x00\x00\x00\x20\x00\x01",
        ),
        Profile::Split2048 => (
            0x400000,
            b"GM T-081586-00",
            b"RA\xe8\x40\x00\x20\x00\x00\x00\x20\x00\x01",
        ),
        Profile::Split8192 => (
            0x400000,
            b"GM T-81576 -00",
            b"RA\xe8\x40\x00\x20\x00\x01\x00\x20\x00\x01",
        ),
        Profile::Switched8 => (
            0x300000,
            b"GM G-5524  -00",
            b"RA\xf8\x20\x00\x20\x00\x01\x00\x20\x3f\xff",
        ),
        Profile::Switched32 => (
            0x300000,
            b"GM MK-1239 -00",
            b"RA\xf8\x20\x00\x20\x00\x01\x00\x20\xff\xff",
        ),
        Profile::Time8 => (
            0x300000,
            b"GM MK-1354 -00",
            b"RA\xf8\x20\x00\x20\x00\x01\x00\x20\x3f\xff",
        ),
        Profile::Sf004 => (0x200000, b"SF-004        ", b"            "),
    };
    let mut rom = vec![0x11; size];
    rom[..0x200].fill(b' ');
    rom[..8].copy_from_slice(&[0, 0xff, 0xff, 0xfc, 0, 0, 2, 0]);
    rom[0x100..0x110].copy_from_slice(b"SEGA MEGA DRIVE ");
    rom[0x180..0x18e].copy_from_slice(product);
    rom[0x1a0..0x1a8].copy_from_slice(&[0, 0, 0, 0, 0, 3, 0xff, 0xff]);
    rom[0x1b0..0x1bc].copy_from_slice(declaration);
    rom[0x1f0] = b'U';
    write_words(&mut rom, 0x200, &[0x4ef9, 0x00ff, 0x1000]);
    write_words(&mut rom, 0x1000, &[0x7001, 0x0640, 3, 0x60f8]);
    write_words(
        &mut rom,
        BANK_SOURCE as usize % size,
        &[0x7001, 0x0640, 3, 0x60f8],
    );
    rom
}

fn machine(profile: Profile, program: &[u16], region: Region, concurrent: bool) -> Emulator {
    let mut machine = Emulator::new(save_image(profile), config(region, concurrent)).unwrap();
    write_words(&mut machine.bus.ram, 0x1000, program);
    machine.step_instruction().unwrap();
    machine
}

fn write_byte(program: &mut Vec<u16>, address: u32, value: u8) {
    program.extend([
        0x13fc,
        u16::from(value),
        (address >> 16) as u16,
        address as u16,
    ]);
}

fn select_sf004(machine: &mut Emulator, bank: u8, expanded: bool) {
    let board = machine.bus.cartridge.sf004.as_mut().unwrap();
    assert!(board.write(0xf01, bank << 4));
    assert!(board.write(0xe01, if expanded { 0xc0 } else { 0x80 }));
}

fn advance(machine: &mut Emulator, ticks: u64) {
    ScheduledBus::new(
        &mut machine.bus,
        &mut machine.sound_cpu,
        &mut machine.sound_schedule,
    )
    .advance_machine(ticks)
    .unwrap();
}

fn restore(receiver: &mut Emulator, raw: &[u8], frontend: &[u8], front: bool) {
    if front {
        receiver.decode_frontend_state(frontend).unwrap();
        assert_eq!(receiver.encode_frontend_state().unwrap(), frontend);
    } else {
        receiver.decode_state(raw).unwrap();
        assert_eq!(receiver.encode_state().unwrap(), raw);
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn natural_save_profiles_and_sf004_keep_source_identity_and_precise_rejection() {
    for profile in NATURAL {
        let rom = save_image(profile);
        let mut machine = machine(profile, &[0x60fe, 0x4e71], Region::NtscUs, true);
        assert!(machine.bus.cartridge.save.is_some() || machine.bus.cartridge.sf004.is_some());
        assert!(machine.supports_rom_cheats(), "{profile:?}");
        let source = machine.bus.cartridge.bytes.clone();
        let digest = machine.bus.cartridge.rom_digest();
        let header = machine.bus.cartridge.header.clone();
        let last = source.len() as u32 - 1;
        machine
            .install_rom_patches(&[patch(0x1000, 0xa5), patch(last, 0x5a)])
            .unwrap();
        assert_eq!(machine.peek(0x1000), Some(0xa5));
        assert!(Arc::ptr_eq(&source, &machine.bus.cartridge.bytes));
        assert_eq!(machine.bus.cartridge.rom_digest(), digest);
        assert_eq!(machine.bus.cartridge.header, header);
        let before = machine.clone();
        for bad in [last + 1, 0x800000, 0xff0000, u32::MAX] {
            assert!(
                machine
                    .install_rom_patches(&[patch(0x1000, 0), patch(bad, 1)])
                    .is_err()
            );
            assert_eq!(machine, before);
        }
        machine.clear_rom_patches();
        assert_eq!(machine.peek(0x1000), Some(0x70));
        assert!(
            Emulator::new_with_timing(rom, config(Region::NtscUs, true), TimingModel::Precise)
                .is_err()
        );
    }
}
