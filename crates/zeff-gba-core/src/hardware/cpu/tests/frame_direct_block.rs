use super::frame_direct::{assert_cpu_equal, assert_projected_equal};
use super::frame_mixed::{CODE_BASES, fixture, oracle_step};

fn block_raw(pre: bool, up: bool, writeback: bool, load: bool, rn: u32, list: u16) -> u32 {
    0xE800_0000
        | (u32::from(pre) << 24)
        | (u32::from(up) << 23)
        | (u32::from(writeback) << 21)
        | (u32::from(load) << 20)
        | (rn << 16)
        | u32::from(list)
}

#[test]
fn frame_direct_block_matches_staged_ram_transfers() {
    for code_base in CODE_BASES {
        for data_base in [0x0200_1201, 0x0300_1201] {
            for pre in [false, true] {
                for up in [false, true] {
                    for writeback in [false, true] {
                        for load in [false, true] {
                            let raw = block_raw(pre, up, writeback, load, 4, 0x25A5);
                            let mut phase = fixture(code_base, &[raw]);
                            phase.cpu.regs[4] = data_base;
                            let mut direct = phase.clone();
                            direct.bus.begin_frame_service();
                            assert!(
                                oracle_step(&mut phase, &mut direct),
                                "{code_base:08X} {data_base:08X} {raw:08X}"
                            );
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn frame_direct_block_matches_every_count_base_and_direction() {
    for base_register in 0..16u32 {
        for count in 1..=15u32 {
            let list = ((1u32 << count) - 1) as u16;
            for load in [false, true] {
                let raw = block_raw(false, true, true, load, base_register, list);
                let mut phase = fixture(0x0800_0200, &[raw]);
                if base_register != 15 {
                    phase.cpu.regs[base_register as usize] = 0x0200_1200;
                }
                let mut direct = phase.clone();
                direct.bus.begin_frame_service();
                assert_eq!(
                    oracle_step(&mut phase, &mut direct),
                    base_register != 15,
                    "rn={base_register} count={count} load={load}"
                );
            }
        }
    }
}

#[test]
fn frame_direct_block_preserves_base_alias_and_prefetched_ram() {
    for (list, expected_stored_base) in [(0x001C, 0x0300_1200), (0x0017, 0x0300_1210)] {
        let raw = block_raw(false, true, true, false, 2, list);
        let mut phase = fixture(0x0300_0200, &[raw]);
        phase.cpu.regs[2] = 0x0300_1200;
        let mut direct = phase.clone();
        direct.bus.begin_frame_service();
        assert!(oracle_step(&mut phase, &mut direct));
        let index = list.trailing_zeros();
        let base_index = (list & ((1 << 2) - 1)).count_ones();
        assert_eq!(
            direct.bus.read32(0x0300_1200 + base_index * 4),
            expected_stored_base,
            "first={index} list={list:04X}"
        );
    }

    let raw = block_raw(false, true, true, true, 2, 0x001C);
    let mut phase = fixture(0x0800_0200, &[raw]);
    phase.cpu.regs[2] = 0x0200_1200;
    let loaded_base = phase.bus.read32(0x0200_1200);
    let mut direct = phase.clone();
    direct.bus.begin_frame_service();
    assert!(oracle_step(&mut phase, &mut direct));
    assert_eq!(direct.cpu.regs[2], loaded_base);

    let raw = block_raw(false, true, true, false, 4, 0x0180);
    let mut phase = fixture(0x0300_0200, &[raw]);
    phase.cpu.regs[4] = 0x0300_0208;
    phase.cpu.regs[7] = 0xE287_7001;
    phase.cpu.regs[8] = 0xE288_8001;
    let mut direct = phase.clone();
    direct.bus.begin_frame_service();
    assert!(oracle_step(&mut phase, &mut direct));
    assert!(
        direct
            .cpu
            .pipeline
            .entries
            .iter()
            .all(|fetched| fetched.raw == 0xE1A0_0000)
    );
    assert_eq!(direct.bus.read32(0x0300_0208), 0xE287_7001);
    assert_eq!(direct.bus.read32(0x0300_020C), 0xE288_8001);
    assert_projected_equal(&phase, &direct);
}

#[test]
fn frame_direct_block_rejects_special_and_crossing_transfers() {
    let cases = [
        (block_raw(false, true, true, false, 4, 0), 0x0200_1200),
        (block_raw(false, true, true, false, 4, 1 << 15), 0x0200_1200),
        (
            block_raw(false, true, true, false, 4, 3) | (1 << 22),
            0x0200_1200,
        ),
        (block_raw(false, true, true, false, 15, 3), 0x0200_1200),
        (block_raw(false, true, true, false, 4, 3), 0x0203_FFFC),
        (block_raw(false, true, true, false, 4, 3), 0x03FF_FFFC),
    ];
    for (raw, address) in cases {
        let mut direct = fixture(0x0300_0200, &[raw]);
        direct.cpu.regs[4] = address;
        direct.bus.begin_frame_service();
        let before = direct.clone();
        assert!(
            direct
                .cpu
                .run_frame_direct(&mut direct.bus, u64::MAX, true, true)
                .is_none(),
            "{raw:08X} {address:08X}"
        );
        assert_cpu_equal(&before.cpu, &direct.cpu);
        assert_eq!(before.bus.ewram, direct.bus.ewram);
        assert_eq!(before.bus.iwram, direct.bus.iwram);
    }
}

#[test]
fn frame_direct_block_accepts_exact_physical_ends_without_wrapping() {
    for address in [0x0203_FFF8, 0x0300_7FF8] {
        let raw = block_raw(false, true, true, false, 4, 3);
        let mut phase = fixture(0x0800_0200, &[raw]);
        phase.cpu.regs[4] = address;
        let mut direct = phase.clone();
        direct.bus.begin_frame_service();
        assert!(oracle_step(&mut phase, &mut direct));
    }
    for address in [0x0203_FFFC, 0x0300_7FFC] {
        let raw = block_raw(false, true, true, false, 4, 3);
        let mut direct = fixture(0x0800_0200, &[raw]);
        direct.cpu.regs[4] = address;
        direct.bus.begin_frame_service();
        let before = direct.clone();
        assert!(
            direct
                .cpu
                .run_frame_direct(&mut direct.bus, u64::MAX, true, true)
                .is_none()
        );
        assert_cpu_equal(&before.cpu, &direct.cpu);
    }
}

#[test]
fn frame_direct_block_obeys_the_exact_guard() {
    let raw = block_raw(false, true, true, true, 4, 0x25A5);
    let mut phase = fixture(0x0800_0200, &[raw]);
    phase.cpu.regs[4] = 0x0200_1200;
    let before = phase.clone();
    let expected = phase.cpu.step(&mut phase.bus).unwrap();
    let exact_guard = phase.cpu.cycles;

    let mut short = before.clone();
    short.bus.begin_frame_service();
    assert!(
        short
            .cpu
            .run_frame_direct(&mut short.bus, exact_guard - 1, true, true)
            .is_none()
    );
    assert_cpu_equal(&before.cpu, &short.cpu);

    let mut exact = before;
    exact.bus.begin_frame_service();
    assert_eq!(
        exact
            .cpu
            .run_frame_direct(&mut exact.bus, exact_guard, true, true),
        Some((expected, 1))
    );
    assert_projected_equal(&phase, &exact);
}
