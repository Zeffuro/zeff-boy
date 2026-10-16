#[path = "save_capacity/domains.rs"]
mod domains;
#[path = "save_capacity/fixture.rs"]
mod fixture;
#[path = "save_capacity/state.rs"]
mod state;

use fixture::{Rom, all_cells_rom, config, expected, run_to_phase};
use zeff_md_core::{Emulator, Region};

#[test]
fn separate_guest_fill_and_read_passes_cover_every_cell_twice() {
    for size in [512, 8192, 32768] {
        let bytes = all_cells_rom(size);
        assert!(bytes.len() < 0x1000);
        for region in [Region::NtscUs, Region::NtscJapan, Region::Pal] {
            for concurrent in [false, true] {
                let mut machine = Emulator::new(bytes.clone(), config(region, concurrent)).unwrap();
                for (phase, seed) in [(1, 0x19), (2, 0xe6)] {
                    run_to_phase(&mut machine, phase);
                    let cells = expected(size, seed);
                    assert_eq!(machine.dump_save_ram().as_deref(), Some(cells.as_slice()));
                    for (index, value) in cells.into_iter().enumerate() {
                        assert_eq!(machine.peek(0xff0000 + index as u32), Some(value));
                        assert_eq!(machine.peek(0x200001 + index as u32 * 2), Some(value));
                        assert_eq!(machine.peek(0x200000 + index as u32 * 2), Some(0xff));
                    }
                }
            }
        }
    }
}

#[test]
fn movep_word_long_wrong_lanes_and_capacity_boundaries_are_ordered() {
    for size in [512, 8192, 32768] {
        let mut rom = Rom::new(size);
        rom.enable();
        let mut cells = vec![0xff; size];
        let mut groups = vec![0, 508];
        if size >= 8192 {
            groups.extend([512, 8188]);
        }
        if size == 32768 {
            groups.extend([8192, 32764]);
        }
        for (group, &index) in groups.iter().enumerate() {
            rom.word(0x207c);
            rom.long(0x200001 + index as u32 * 2);
            rom.word(0x203c);
            rom.long(0x136a9ce7 ^ (group as u32 * 0x01010101));
            rom.word(0x01c8);
            rom.word(0);
            rom.word(0x0148);
            rom.word(0);
            rom.word(0x23c0);
            rom.long(0xff0100 + group as u32 * 4);
            cells[index..index + 4]
                .copy_from_slice(&(0x136a9ce7 ^ (group as u32 * 0x01010101)).to_be_bytes());
        }
        rom.write_word(0x200000, 0x1234);
        rom.write_long(0x200002, 0x56789abc);
        cells[..3].copy_from_slice(&[0x34, 0x78, 0xbc]);
        let last_even = 0x200000 + size as u32 * 2 - 2;
        rom.write_long(last_even, 0x11223344);
        cells[size - 1] = 0x22;
        rom.byte(0x200000, 0x99);
        rom.byte(last_even, 0x99);
        for (index, address) in [
            0x200000,
            0x200001,
            0x200003,
            0x200005,
            last_even,
            last_even + 1,
            last_even + 2,
            last_even + 3,
        ]
        .into_iter()
        .enumerate()
        {
            rom.read_byte(address, 0xff0000 + index as u32);
        }
        rom.word(0x3039);
        rom.long(0x200000);
        rom.word(0x33c0);
        rom.long(0xff0020);
        rom.word(0x2039);
        rom.long(last_even);
        rom.word(0x23c0);
        rom.long(0xff0022);
        let mut machine = Emulator::new(rom.finish(), config(Region::NtscUs, true)).unwrap();
        run_to_phase(&mut machine, 3);
        assert_eq!(machine.dump_save_ram(), Some(cells));
        for (index, value) in [0xff, 0x34, 0x78, 0xbc, 0xff, 0x22, 0xff, 0xff]
            .into_iter()
            .enumerate()
        {
            assert_eq!(machine.peek(0xff0000 + index as u32), Some(value));
        }
        for (group, _) in groups.iter().enumerate() {
            let expected = (0x136a9ce7 ^ (group as u32 * 0x01010101)).to_be_bytes();
            for (offset, byte) in expected.into_iter().enumerate() {
                assert_eq!(
                    machine.peek(0xff0100 + group as u32 * 4 + offset as u32),
                    Some(byte)
                );
            }
        }
        for (offset, value) in [0xff, 0x34, 0xff, 0x22, 0xff, 0xff].into_iter().enumerate() {
            assert_eq!(machine.peek(0xff0020 + offset as u32), Some(value));
        }
    }
}
