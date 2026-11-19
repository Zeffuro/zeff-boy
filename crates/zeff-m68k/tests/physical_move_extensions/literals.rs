use super::*;

#[test]
fn fifteen_literal_odd_frames_match_word_order_offsets_and_total_elapsed() {
    use Family::{
        AbsoluteLong as AL, AbsoluteWord as AW, Displacement as D, Indexed as I,
        PcDisplacement as PD, PcIndexed as PI,
    };
    use Size::{Long as L, Word as W};
    let rows = [
        (
            W,
            I,
            false,
            0,
            1,
            0x00b0_ba82_u32,
            0xf82a_75f5,
            0x2119,
            1,
            [0xba84, 0x2119, 0x00b0, 0x3031, 0x75f5, 0x3035, 0xf82a],
            64,
        ),
        (
            W,
            AL,
            false,
            2,
            0,
            0x005b_c8d2,
            0xd432_62ef,
            0x2710,
            1,
            [0xc8d8, 0x2710, 0x005b, 0x3439, 0x62ef, 0x3435, 0xd432],
            66,
        ),
        (
            W,
            D,
            true,
            2,
            0,
            0x00e9_07d6,
            0xef3f_2cf1,
            0x2510,
            1,
            [0x07da, 0x2510, 0x00e9, 0x3142, 0x2cf1, 0x3145, 0xef3f],
            62,
        ),
        (
            W,
            I,
            true,
            3,
            7,
            0x0088_e990,
            0x007e_f1d3,
            0x2000,
            1,
            [0xe994, 0x2000, 0x0088, 0x3f83, 0xf1d3, 0x3f85, 0x007e],
            64,
        ),
        (
            W,
            D,
            false,
            0,
            1,
            0x00a8_5cf8,
            0x4464_3eff,
            0x271d,
            1,
            [0x5cfa, 0x271d, 0x00a8, 0x3029, 0x3eff, 0x3035, 0x4464],
            62,
        ),
        (
            W,
            AW,
            true,
            1,
            0,
            0x0032_8bb2,
            0xffff_cb37,
            0x0118,
            0x8001,
            [0x8bb6, 0x0118, 0x0032, 0x31c1, 0xcb37, 0x31c1, 0xffff],
            62,
        ),
        (
            W,
            AL,
            true,
            1,
            0,
            0x0035_2de2,
            0xfa88_85e1,
            0x2518,
            0x8001,
            [0x2de8, 0x2518, 0x0035, 0x33c1, 0x85e1, 0x33c5, 0xfa88],
            66,
        ),
        (
            L,
            I,
            true,
            7,
            1,
            0x0097_1bc2,
            0x9236_bd6d,
            0x211a,
            0x5a48_ca3c,
            [0x1bc6, 0x2112, 0x0097, 0x2387, 0xbd6d, 0x2385, 0x9236],
            64,
        ),
        (
            L,
            D,
            true,
            7,
            4,
            0x0003_4ab2,
            0x06bc_abad,
            0x2719,
            0xed74_2064,
            [0x4ab6, 0x2719, 0x0003, 0x2947, 0xabad, 0x2945, 0x06bc],
            62,
        ),
        (
            L,
            D,
            false,
            1,
            7,
            0x00ae_38fa,
            0x00aa_7517,
            0x0502,
            1,
            [0x38fc, 0x0502, 0x00ae, 0x222f, 0x7517, 0x2231, 0x00aa],
            62,
        ),
        (
            L,
            AL,
            true,
            3,
            0,
            0x0019_c996,
            0x6863_0b5f,
            0x201f,
            0x68fa_b028,
            [0xc99c, 0x2010, 0x0019, 0x23c3, 0x0b5f, 0x23c5, 0x6863],
            66,
        ),
        (
            L,
            I,
            false,
            3,
            0,
            0x001a_82ac,
            0x21eb_4ec3,
            0x0504,
            1,
            [0x82ae, 0x0504, 0x001a, 0x2630, 0x4ec3, 0x2631, 0x21eb],
            64,
        ),
        (
            L,
            AW,
            true,
            1,
            0,
            0x0026_e582,
            0xffff_de15,
            0x0010,
            1,
            [0xe586, 0x0010, 0x0026, 0x21c1, 0xde15, 0x21c1, 0xffff],
            62,
        ),
        (
            L,
            PD,
            false,
            2,
            0,
            0x00a0_49bc,
            0x00a0_2c4b,
            0x0219,
            1,
            [0x49be, 0x0219, 0x00a0, 0x243a, 0x2c4b, 0x2432, 0x00a0],
            62,
        ),
        (
            L,
            PI,
            false,
            2,
            0,
            0x00e8_5a66,
            0xb556_f243,
            0x2116,
            1,
            [0x5a68, 0x2116, 0x00e8, 0x243b, 0xf243, 0x2436, 0xb556],
            64,
        ),
    ];
    for (size, family, write, dn, an, pc, ea, sr, value, words, clocks) in rows {
        let form = Form {
            size,
            family,
            write,
        };
        let mut case = Case::new(form);
        case.r = registers(sr & SUPERVISOR != 0, 0);
        case.r.pc = pc;
        case.r.set_status_register(sr);
        case.value = value;
        case.dn = dn;
        case.an = an;
        match family {
            D => {
                case.e0 = 1;
                assert!(case.r.set_address_register(usize::from(an), ea - 1));
            }
            I => {
                case.e0 = 0x4801;
                case.r.d[4] = 0;
                assert!(case.r.set_address_register(usize::from(an), ea - 1));
            }
            AW => case.e0 = ea as u16,
            AL => {
                case.e0 = (ea >> 16) as u16;
                case.e4 = ea as u16;
            }
            PD => case.e0 = ea.wrapping_sub(pc + 2) as u16,
            PI => {
                case.e0 = 0x4800;
                case.r.d[4] = ea.wrapping_sub(pc + 2);
            }
            _ => unreachable!(),
        }
        assert_eq!(form.opcode(dn, an), words[3]);
        let (mut cpu, mut bus, case) = case.seed();
        bus.detection = Some((Failure::Boundary, 8));
        let plan = oracle::plan(case, &bus.words);
        let at = plan
            .iter()
            .position(|t| t.access.kind == AccessKind::Data)
            .unwrap();
        let capture = oracle::capture(plan[at], 3);
        assert_eq!(capture.logical_address, ea);
        assert_eq!(
            cpu.step_physical_instruction(&mut bus),
            Ok(PhysicalInstructionStep::Group0 {
                opcode: words[3],
                capture,
                clocks
            })
        );
        let actual: Vec<_> = bus
            .events
            .iter()
            .filter_map(|event| {
                if let Event::EntryWrite(_, value, _) = event {
                    Some(*value)
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(actual, words);
        oracle::frame(&bus, case.r.ssp, capture, case.start + clocks - 50);
        assert_eq!(cpu.registers, oracle::entered(plan[at].registers));
        assert_eq!(cpu.clock().0, case.start + clocks);
        assert_eq!(cpu.clock().0, bus.clock);
    }
}
