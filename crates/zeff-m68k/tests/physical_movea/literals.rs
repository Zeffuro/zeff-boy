use super::*;

#[test]
fn all_eighteen_literal_first_odd_frames_match_seven_writes_and_raw_totals() {
    use Family::{
        AbsoluteLong as AL, AbsoluteWord as AW, Displacement as D, Indexed as I, Indirect as R,
        PcDisplacement as PD, PcIndexed as PI, Post, Pre,
    };
    use Size::{Long as L, Word as W};
    let rows = [
        (
            W,
            R,
            0,
            4,
            0x00e8_919a_u32,
            0x0d87_e38f_u32,
            0x2415,
            [0x919c, 0x2415, 0x00e8, 0x3054, 0xe38f, 0x3055, 0x0d87],
            58,
        ),
        (
            W,
            Post,
            4,
            3,
            0x00c4_abac,
            0x01fa_b845,
            0x0303,
            [0xabae, 0x0303, 0x00c4, 0x385b, 0xb845, 0x3851, 0x01fa],
            58,
        ),
        (
            W,
            Pre,
            6,
            3,
            0x00ff_4c2e,
            0xb8b4_5951,
            0x230d,
            [0x4c32, 0x230d, 0x00ff, 0x3c63, 0x5951, 0x3c75, 0xb8b4],
            60,
        ),
        (
            W,
            D,
            7,
            4,
            0x008e_bbb2,
            0x4558_b0a9,
            0x0119,
            [0xbbb4, 0x0119, 0x008e, 0x3e6c, 0xb0a9, 0x3e71, 0x4558],
            62,
        ),
        (
            W,
            I,
            7,
            6,
            0x005e_3380,
            0x1bec_9d67,
            0x2208,
            [0x3382, 0x2208, 0x005e, 0x3e76, 0x9d67, 0x3e75, 0x1bec],
            64,
        ),
        (
            W,
            AW,
            1,
            0,
            0x00fb_e614,
            0xffff_eac3,
            0x271f,
            [0xe618, 0x271f, 0x00fb, 0x3278, 0xeac3, 0x3275, 0xffff],
            62,
        ),
        (
            W,
            AL,
            1,
            0,
            0x000c_c614,
            0x9022_8f01,
            0x2416,
            [0xc61a, 0x2416, 0x000c, 0x3279, 0x8f01, 0x3275, 0x9022],
            66,
        ),
        (
            W,
            PD,
            7,
            0,
            0x009e_24be,
            0x009e_5049,
            0x010f,
            [0x24c0, 0x010f, 0x009e, 0x3e7a, 0x5049, 0x3e72, 0x009e],
            62,
        ),
        (
            W,
            PI,
            1,
            0,
            0x00bd_f254,
            0xfc24_3339,
            0x0612,
            [0xf256, 0x0612, 0x00bd, 0x327b, 0x3339, 0x3272, 0xfc24],
            64,
        ),
        (
            L,
            R,
            0,
            5,
            0x0007_6674,
            0x0524_9de3,
            0x2511,
            [0x6676, 0x2511, 0x0007, 0x2055, 0x9de3, 0x2055, 0x0524],
            58,
        ),
        (
            L,
            Post,
            3,
            6,
            0x00e6_9af4,
            0x30dd_b463,
            0x040b,
            [0x9af6, 0x040b, 0x00e6, 0x265e, 0xb463, 0x2651, 0x30dd],
            58,
        ),
        (
            L,
            Pre,
            6,
            1,
            0x00a8_7f90,
            0x48fe_5c2f,
            0x2509,
            [0x7f92, 0x2509, 0x00a8, 0x2c61, 0x5c2f, 0x2c75, 0x48fe],
            60,
        ),
        (
            L,
            D,
            5,
            3,
            0x0071_ebe6,
            0x6e47_e453,
            0x2719,
            [0xebe8, 0x2719, 0x0071, 0x2a6b, 0xe453, 0x2a75, 0x6e47],
            62,
        ),
        (
            L,
            I,
            6,
            7,
            0x001e_0102,
            0x00d4_5e15,
            0x001a,
            [0x0104, 0x001a, 0x001e, 0x2c77, 0x5e15, 0x2c71, 0x00d4],
            64,
        ),
        (
            L,
            AW,
            4,
            0,
            0x0061_f4f4,
            0xffff_f8f5,
            0x010f,
            [0xf4f8, 0x010f, 0x0061, 0x2878, 0xf8f5, 0x2871, 0xffff],
            62,
        ),
        (
            L,
            AL,
            4,
            0,
            0x004e_f8ae,
            0xb8e2_7ef9,
            0x0005,
            [0xf8b4, 0x0005, 0x004e, 0x2879, 0x7ef9, 0x2871, 0xb8e2],
            66,
        ),
        (
            L,
            PD,
            3,
            0,
            0x0039_ec38,
            0x0039_7ee5,
            0x0413,
            [0xec3a, 0x0413, 0x0039, 0x267a, 0x7ee5, 0x2672, 0x0039],
            62,
        ),
        (
            L,
            PI,
            2,
            0,
            0x0044_421e,
            0x0044_2b69,
            0x000c,
            [0x4220, 0x000c, 0x0044, 0x247b, 0x2b69, 0x2472, 0x0044],
            64,
        ),
    ];
    for (size, family, destination, source, pc, ea, sr, words, clocks) in rows {
        let form = Form { size, family };
        let mut case = Case::new(form);
        case.r = registers(sr & SUPERVISOR != 0, 0);
        case.r.set_status_register(sr);
        case.r.pc = pc;
        case.destination = destination;
        case.source = source;
        let base = match family {
            R | Post => Some(ea),
            Pre => Some(ea.wrapping_add(if size == L { 4 } else { 2 })),
            D => {
                case.e0 = 1;
                Some(ea - 1)
            }
            I => {
                case.e0 = 0x4801;
                case.r.d[4] = 0;
                Some(ea - 1)
            }
            AW => {
                case.e0 = ea as u16;
                None
            }
            AL => {
                case.e0 = (ea >> 16) as u16;
                case.e4 = ea as u16;
                None
            }
            PD => {
                case.e0 = ea.wrapping_sub(pc + 2) as u16;
                None
            }
            PI => {
                case.e0 = 0x4800;
                case.r.d[4] = ea.wrapping_sub(pc + 2);
                None
            }
            _ => unreachable!(),
        };
        if let Some(base) = base {
            assert!(case.r.set_address_register(usize::from(source), base));
        }
        assert_eq!(form.opcode(destination, source), words[3]);
        let (mut cpu, mut bus, case) = case.seed();
        let memory = bus.words.clone();
        let plan = oracle::plan(case, &memory);
        let at = plan
            .iter()
            .position(|t| t.access.kind == AccessKind::Data)
            .unwrap();
        let t = plan[at];
        let capture = oracle::capture(t, 3);
        assert_eq!(capture.logical_address, ea);
        bus.detection = Some((Failure::Boundary, 8));
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
            .filter_map(|e| {
                if let Event::EntryWrite(_, v, _) = e {
                    Some(*v)
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(actual, words);
        assert_eq!(cpu.registers, oracle::entered(t.registers));
        assert_eq!(
            bus.words,
            oracle::entry_memory(memory, t.registers.ssp, capture)
        );
        oracle::frame(&bus, t.registers.ssp, capture, case.start + clocks - 50);
        assert_eq!(cpu.clock().0, case.start + clocks);
        assert_eq!(cpu.clock().0, bus.clock);
    }
}
