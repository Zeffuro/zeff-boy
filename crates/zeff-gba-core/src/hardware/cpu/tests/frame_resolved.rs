use super::frame_mixed::{fixture, oracle_step};

#[test]
fn frame_resolved_transfers_preserve_final_slots_alignment_and_mirrors() {
    for (load, instructions) in [
        (false, [0xE5C0_1000, 0xE1C0_10B0, 0xE580_1000]),
        (true, [0xE5D0_1000, 0xE1D0_10B0, 0xE590_1000]),
    ] {
        for raw in instructions {
            for address in [
                0x0203_FFFCu32,
                0x0207_FFFC,
                0x02FF_FFFC,
                0x0300_7FFC,
                0x0300_FFFC,
                0x03FF_FFFC,
                0x0800_3FFC,
                0x0A00_3FFC,
                0x0C00_3FFC,
                0x0800_4000,
            ] {
                for alignment in 0..4 {
                    let address = address + alignment;
                    let mut phase = fixture(0x0300_0200, &[raw]);
                    phase.cpu.regs[0] = address;
                    phase.cpu.regs[1] = 0x89AB_CDEF;
                    phase.bus.write32(address & !3, 0x81F2_7354);
                    let mut direct = phase.clone();
                    direct.bus.begin_frame_service();
                    assert_eq!(
                        oracle_step(&mut phase, &mut direct),
                        address < 0x0800_0000 || (load && (address & 0x01FF_FFFF) < 0x4000),
                        "{raw:08X} {address:08X}"
                    );
                }
            }
        }
    }
}

#[test]
fn frame_resolved_transfers_keep_debug_reads_and_writes_on_scalar_path() {
    for raw in [
        0xE5C0_1000,
        0xE1C0_10B0,
        0xE580_1000,
        0xE5D0_1000,
        0xE1D0_10B0,
        0xE590_1000,
    ] {
        let mut phase = fixture(0x0300_0200, &[raw]);
        phase.bus.debug_trace_enabled = true;
        phase.bus.debug_trace_reads = true;
        phase.bus.debug_trace_writes = true;
        let mut direct = phase.clone();
        direct.bus.begin_frame_service();
        assert!(!oracle_step(&mut phase, &mut direct));
        assert!(!direct.bus.debug_trace_events.borrow().is_empty());
    }
}

#[test]
fn frame_resolved_transfers_reject_rtc_and_eeprom_cartridge_reads() {
    use crate::hardware::cartridge::Cartridge;
    let mut rom = vec![0; 0x4000];
    rom[0xA0..0xA4].copy_from_slice(b"TEST");
    rom[0xAC..0xB0].copy_from_slice(b"BPEE");
    rom[0xB2] = 0x96;
    for raw in [0xE5D0_1000, 0xE1D0_10B0, 0xE590_1000] {
        for address in [0x0800_00C4, 0x0800_3000, 0x0A00_3000, 0x0C00_3000] {
            let mut phase = fixture(0x0300_0200, &[raw]);
            phase.bus.cartridge = Cartridge::load(&rom).unwrap();
            phase.cpu.regs[0] = address;
            let mut direct = phase.clone();
            direct.bus.begin_frame_service();
            assert!(!oracle_step(&mut phase, &mut direct));
        }
    }
    rom[0xAC..0xB0].fill(0);
    rom[0x180..0x188].copy_from_slice(b"EEPROM_V");
    for raw in [0xE5D0_1000, 0xE1D0_10B0, 0xE590_1000] {
        let mut phase = fixture(0x0300_0200, &[raw]);
        phase.bus.cartridge = Cartridge::load(&rom).unwrap();
        phase.cpu.regs[0] = 0x0D00_0000;
        assert!(phase.bus.cartridge.is_eeprom_access_addr(phase.cpu.regs[0]));
        let mut direct = phase.clone();
        direct.bus.begin_frame_service();
        assert!(!oracle_step(&mut phase, &mut direct));
    }
}
