use super::*;
use zeff_emu_common::save_state::StateWriter;
use zeff_m68k::bus::FunctionCode;
use zeff_m68k::timing::PrefetchCpu;

fn access(address: u32, lanes: ByteLanes) -> BusAccess {
    BusAccess {
        logical_address: address,
        bus_address: address & 0xfffffe,
        lanes,
        kind: AccessKind::Data,
        function_code: FunctionCode::SupervisorData,
    }
}

fn fixture(region: Region, program: &[u16], instructions: usize) -> Bus {
    let mut rom = vec![0; 0x1000];
    rom[..8].copy_from_slice(&[0, 0xff, 0xff, 0xfc, 0, 0, 2, 0]);
    for (index, word) in program.iter().enumerate() {
        rom[0x200 + index * 2..0x202 + index * 2].copy_from_slice(&word.to_be_bytes());
    }
    let mut bus = Bus::new_owned(Cartridge::new(rom).unwrap(), region).unwrap();
    let mut cpu = PrefetchCpu::default();
    cpu.registers = bus.cartridge.effective_initial();
    cpu.prime(&mut bus.owned_context().unwrap()).unwrap();
    for _ in 0..instructions {
        cpu.step_boundary(&mut bus.owned_context().unwrap())
            .unwrap();
    }
    bus
}

fn raw_bus(bus: &Bus) -> Vec<u8> {
    let mut writer = StateWriter::new();
    bus.write_state(&mut writer).unwrap();
    writer.into_bytes()
}

fn observed_write(bus: &mut Bus, access: BusAccess, value: u16) -> Witness {
    let mut expected = bus.clone();
    let start = bus.cpu_clock;
    let normal = expected
        .owned_context()
        .unwrap()
        .write_cycle(access, value, start);
    let (actual, witness) = bus
        .owned_context()
        .unwrap()
        .write_with_work_witness(access, value, start);
    assert_eq!(actual, normal);
    actual.result.unwrap();
    assert_eq!(*bus, expected);
    assert_eq!(raw_bus(bus), raw_bus(&expected));
    witness.check_links();
    witness
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn precise_first_work_ram_pulse_has_main_identity_and_original_storage() {
    for region in [Region::NtscUs, Region::NtscJapan, Region::Pal] {
        let base = fixture(region, &[0x60fe, 0x4e71], 0);
        for address in [0xff3ffe, 0xff4000, 0xff7ffe, 0xff8000, 0xe08000] {
            for lanes in [ByteLanes::Upper, ByteLanes::Lower, ByteLanes::Both] {
                let mut bus = base.clone();
                let seed = access(address, ByteLanes::Both);
                let start = bus.cpu_clock;
                bus.owned_context()
                    .unwrap()
                    .write_cycle(seed, 0x1122, start)
                    .result
                    .unwrap();
                let logical = address | u32::from(lanes == ByteLanes::Lower);
                let request = access(logical, lanes);
                let witness = observed_write(&mut bus, request, 0x3344);
                witness.check_main_suffix(0, request, 0x3344, 0x1122);
                let repeated = observed_write(&mut bus, request, 0x3344);
                repeated.check_main_suffix(0, request, 0x3344, 0x3344);
            }
        }
    }
}

fn sound_write_prefix(region: Region) -> Bus {
    let mut program = Vec::new();
    let mut byte = |address: u32, value: u8| {
        program.extend([
            0x13fc,
            u16::from(value),
            (address >> 16) as u16,
            address as u16,
        ]);
    };
    byte(0xff8000, 0x11);
    byte(0xa11100, 1);
    byte(0xa11200, 1);
    for _ in 0..9 {
        byte(0xa06000, 1);
    }
    let mut sound = vec![0; 8];
    sound.extend([0x21, 0x00, 0x80, 0x3e, 0x22, 0x77, 0x76]);
    for (offset, value) in sound.into_iter().enumerate() {
        byte(0xa00000 + offset as u32, value);
    }
    byte(0xa11100, 0);
    let instructions = program.len() / 4;
    program.extend([0x60fe, 0x4e71]);
    fixture(region, &program, instructions)
}

fn sound_write_frontier(region: Region) -> Bus {
    let mut bus = sound_write_prefix(region);
    let begin = bus.vdp.owned_half().unwrap();
    let mut found = false;
    // Reuse the existing owned local-service fixture's 4096-half search bound.
    while bus.vdp.owned_half().unwrap() - begin + 14 <= 4096 {
        let owner = bus.owned.as_ref().unwrap();
        let pins = owner.sound.pins();
        if owner.board.work_witness_tristate()
            && pins.shared()
            && pins.write
            && pins.address == Some(0x8000)
            && pins.data == Some(0x22)
            && owner.sound.bank() == 0x1ff
            && bus.ram[0x8000] == 0x11
        {
            found = true;
            break;
        }
        let start = bus.cpu_clock;
        bus.owned_context().unwrap().idle(start, 1);
        assert!(bus.owned_fault().is_none());
    }
    assert!(
        found,
        "no real uncommitted SOUND bank-write grant in the bounded frontier search: {region:?}"
    );
    bus
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn precise_sound_bank_steal_changes_storage_before_first_main_write_pulse() {
    for region in [Region::NtscUs, Region::NtscJapan, Region::Pal] {
        let mut bus = sound_write_frontier(region);
        let entry = bus.ram[0x8000];
        assert_eq!(entry, 0x11);
        let request = access(0xff8000, ByteLanes::Upper);
        let witness = observed_write(&mut bus, request, 0x3300);
        let first_main = witness.check_sound_prefix(request, entry);
        witness.check_main_suffix(first_main, request, 0x3300, 0x2200);
        assert_ne!(entry, 0x22);
        assert_eq!(bus.ram[0x8000], 0x33);
    }
}

mod previous;
