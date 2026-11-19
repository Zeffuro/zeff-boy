use super::*;

#[test]
fn twelve_literal_raw_frames_one_capture_gap_and_one_absword_synthetic_frame() {
    use Family::{
        AbsoluteLong as AL, AbsoluteWord as AW, Displacement as D, Indexed as I, Indirect as R,
        Post, Pre,
    };
    use Size::{Long as L, Word as W};
    let rows = [
        (
            W,
            R,
            5,
            1,
            0x00d0_1ad2_u32,
            0xbd97_f81d_u32,
            0x2500,
            1,
            [0x1ad6, 0x2500, 0x00d0, 0x328d, 0xf81d, 0x3285, 0xbd97],
            58,
        ),
        (
            W,
            Post,
            1,
            0,
            0x005d_d666,
            0x0416_4129,
            0x2118,
            0x8000,
            [0xd66a, 0x2118, 0x005d, 0x30c9, 0x4129, 0x30c5, 0x0416],
            58,
        ),
        (
            W,
            Pre,
            4,
            5,
            0x004f_280c,
            0xfcf8_68e9,
            0x0108,
            0x8000,
            [0x2810, 0x0108, 0x004f, 0x2dc4, 0x68e9, 0x2dc1, 0xfcf8],
            62,
        ),
        (
            W,
            D,
            5,
            1,
            0x0034_6c3c,
            0xcbe8_f6cf,
            0x2410,
            1,
            [0x6c40, 0x2410, 0x0034, 0x334d, 0xf6cf, 0x3345, 0xcbe8],
            62,
        ),
        (
            W,
            I,
            2,
            1,
            0x0024_fafc,
            0xa936_324f,
            0x2308,
            0x8000,
            [0xfb00, 0x2308, 0x0024, 0x338a, 0x324f, 0x3385, 0xa936],
            64,
        ),
        (
            W,
            AW,
            0,
            0,
            0x0001_0212,
            0xffff_cc07,
            0x2018,
            0x8000,
            [0x0216, 0x2018, 0x0001, 0x31c8, 0xcc07, 0x31c5, 0xffff],
            62,
        ),
        (
            W,
            AL,
            2,
            1,
            0x003d_bb02,
            0xbfa2_01f5,
            0x0010,
            1,
            [0xbb08, 0x0010, 0x003d, 0x33ca, 0x01f5, 0x33c1, 0xbfa2],
            66,
        ),
        (
            L,
            R,
            5,
            1,
            0x0077_38be,
            0xebf1_7779,
            0x261b,
            0x8000_0001,
            [0x38c2, 0x261b, 0x0077, 0x228d, 0x7779, 0x2285, 0xebf1],
            58,
        ),
        (
            L,
            Post,
            0,
            5,
            0x0039_dece,
            0xaa3b_fd1f,
            0x2106,
            0x0001_0000,
            [0xded2, 0x2106, 0x0039, 0x2ac8, 0xfd1f, 0x2ac5, 0xaa3b],
            58,
        ),
        (
            L,
            Pre,
            7,
            2,
            0x0018_56d6,
            0xfbcd_e349,
            0x2200,
            0x1234_8000,
            [0x56da, 0x2200, 0x0018, 0x250f, 0xe349, 0x2505, 0xfbcd],
            62,
        ),
        (
            L,
            D,
            4,
            0,
            0x00e7_499c,
            0xb99a_ae2d,
            0x011a,
            0x8000_0001,
            [0x49a0, 0x011a, 0x00e7, 0x214c, 0xae2d, 0x2141, 0xb99a],
            62,
        ),
        (
            L,
            I,
            2,
            3,
            0x000b_22bc,
            0xca2f_2ec7,
            0x0101,
            0x0001_0000,
            [0x22c0, 0x0101, 0x000b, 0x278a, 0x2ec7, 0x2781, 0xca2f],
            64,
        ),
        (
            L,
            AL,
            4,
            1,
            0x00b2_c82e,
            0x2677_3ce3,
            0x0400,
            1,
            [0xc834, 0x0400, 0x00b2, 0x23cc, 0x3ce3, 0x23c1, 0x2677],
            66,
        ),
        (
            L,
            AW,
            2,
            0,
            0x0000_1000,
            0xffff_9001,
            0x0018,
            0x8000_0001,
            [0x1004, 0x0018, 0x0000, 0x21ca, 0x9001, 0x21c1, 0xffff],
            62,
        ),
    ];
    assert_eq!(rows.len(), 14);
    let (mut frames, mut gaps) = (0, 0);
    for (size, family, source, destination, pc, ea, sr, value, words, clocks) in rows {
        let mut case = Case::new(Form { size, family });
        case.r = registers(sr & SUPERVISOR != 0, 0);
        case.r.set_status_register(sr);
        case.r.pc = pc;
        case.source = source;
        case.destination = destination;
        case.r.set_address_register(usize::from(source), value);
        let base = match family {
            R | Post => Some(ea),
            Pre => {
                if size == W {
                    case.e0 = words[3];
                }
                Some(ea.wrapping_add(2))
            }
            D => {
                case.e0 = 1;
                Some(ea.wrapping_sub(1))
            }
            I => {
                case.e0 = 0x4801;
                case.r.d[4] = 0;
                Some(ea.wrapping_sub(1))
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
            _ => unreachable!(),
        };
        if let Some(base) = base {
            case.r.set_address_register(usize::from(destination), base);
        }
        let (mut cpu, mut bus, case) = case.seed();
        let memory = bus.words.clone();
        let plan = oracle::plan(case, &memory);
        let at = plan.iter().position(|t| t.value.is_some()).unwrap();
        let t = plan[at];
        let c = oracle::capture(t, 3);
        assert_eq!(c.logical_address, ea);
        bus.detection = Some((Failure::Boundary, 8));
        let result = cpu.step_physical_instruction(&mut bus);
        if size == W && family == Pre {
            gaps += 1;
            assert_eq!(c.ird, 0x2dc4);
            assert_eq!(
                [
                    c.internal_pc as u16,
                    c.phase_sr,
                    (c.internal_pc >> 16) as u16,
                    c.ird,
                    c.logical_address as u16,
                    (c.ird & 0xffe0) | c.function_code as u16,
                    (c.logical_address >> 16) as u16,
                ],
                words
            );
            let cause = CriticalFault::AddressError {
                access: t.access,
                direction: Direction::Write,
            };
            assert_eq!(
                result,
                Err(InstructionFault::CaptureUnavailable {
                    phase: t.phase,
                    cause,
                })
            );
            let prefix = oracle::clocks(case, &bus.elapsed, at);
            let consumed = prefix + 8;
            assert_eq!(consumed, 12);
            assert_eq!(consumed, clocks - 50);
            let mut events = oracle::events(case, &plan[..at], &bus.elapsed);
            events.push(Event::Detect(
                t.access,
                Direction::Write,
                case.start + prefix,
            ));
            assert_eq!(bus.events, events);
            assert_eq!(bus.words, oracle::memory(memory, &plan[..at]));
            assert_eq!(bus.count, at);
            assert_eq!(bus.entry_count, 0);
            assert!(
                !bus.events
                    .iter()
                    .any(|event| matches!(event, Event::EntryWrite(..) | Event::Write(..)))
            );
            assert_eq!(cpu.registers, t.registers);
            assert_eq!(cpu.queued_words(), None);
            assert_eq!(cpu.clock().0, case.start + consumed);
            assert_eq!(cpu.clock().0, bus.clock);
            assert_eq!(
                cpu.physical_entry_state(),
                PhysicalEntryState::FaultUnqualified { cause }
            );
            assert_eq!(cpu.physical_inspection(), PhysicalInspection::Unavailable);
            guarded(&mut cpu, &mut bus, TimingFault::PhysicalFaultUnqualified);
            let before = cpu.clone();
            let before_bus = (
                bus.events.clone(),
                bus.words.clone(),
                bus.clock,
                bus.count,
                bus.entry_count,
            );
            assert_eq!(
                cpu.enter_imported_group0(&mut bus),
                Err(EntryFault::FaultUnqualified(cause))
            );
            assert_eq!(cpu, before);
            assert_eq!(
                (
                    bus.events.clone(),
                    bus.words.clone(),
                    bus.clock,
                    bus.count,
                    bus.entry_count
                ),
                before_bus
            );
            continue;
        }
        frames += 1;
        assert_eq!(
            result,
            Ok(PhysicalInstructionStep::Group0 {
                opcode: case.form.opcode(destination, source),
                capture: c,
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
            oracle::entry_memory(oracle::memory(memory, &plan[..at]), t.registers.ssp, c)
        );
        oracle::frame(&bus, t.registers.ssp, c, case.start + clocks - 50);
        assert_eq!(cpu.clock().0, case.start + clocks);
        assert_eq!(cpu.clock().0, bus.clock);
    }
    assert_eq!((frames, gaps), (13, 1));
}
