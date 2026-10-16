use super::*;

#[test]
fn directed_long_immediate_pre_a7_preserves_literal_prefix_flags_ssp_and_queue() {
    let form = Form {
        size: Size::Long,
        family: Family::Pre,
    };
    let mut case = Case::new(form);
    case.r = registers(true, 31);
    case.r.pc = 0x1000;
    case.r.ssp = 0x8000;
    case.r.set_status_register(0x201f);
    case.destination = 7;
    case.value = 0x8000_0001;
    case.low = 1;
    case.last = 0x7402;
    let (mut cpu, mut bus, _) = case.seed();
    let plan = oracle::plan(case, &bus.words);
    assert_eq!(form.opcode(7), 0x2f3c);
    let actual: Vec<_> = plan
        .iter()
        .map(|t| {
            (
                t.access.logical_address,
                t.internal_pc,
                t.ird,
                t.registers.status_register(),
                t.registers.ssp,
                t.value,
            )
        })
        .collect();
    assert_eq!(
        actual,
        vec![
            (0x1004, 0x1002, 0x2f3c, 0x201f, 0x8000, None),
            (0x1006, 0x1006, 0x2f3c, 0x201f, 0x8000, None),
            (0x1008, 0x1008, 0x2f3c, 0x2010, 0x8000, None),
            (0x7ffe, 0x1008, 0x2f3c, 0x2018, 0x8000, Some(1)),
            (0x7ffc, 0x1008, 0x7201, 0x2018, 0x7ffc, Some(0x8000)),
        ]
    );
    let mut legacy_cpu = cpu.clone();
    let mut legacy_bus = Device::new(case.start);
    legacy_bus.words = bus.words.clone();
    assert_eq!(
        legacy_cpu.step_physical_instruction(&mut legacy_bus),
        Ok(PhysicalInstructionStep::Instruction(
            zeff_m68k::timing::TimedStep {
                opcode: 0x2f3c,
                clocks: 20,
                exception: None,
            }
        ))
    );
    assert_eq!(
        cpu.step_physical_instruction(&mut bus),
        Ok(PhysicalInstructionStep::Instruction(
            zeff_m68k::timing::TimedStep {
                opcode: 0x2f3c,
                clocks: 20,
                exception: None
            }
        ))
    );
    let mut expected = case.r;
    expected.pc = 0x1006;
    expected.ssp = 0x7ffc;
    expected.set_status_register(0x2018);
    assert_eq!(cpu.registers, expected);
    assert_eq!(cpu.queued_words(), Some([0x7201, 0x7402]));
    assert_eq!(bus.sample(0x7ffc), 0x8000);
    assert_eq!(bus.sample(0x7ffe), 1);
    assert_eq!(bus.events, oracle::events(case, &plan, &bus.elapsed, true));
    assert_eq!(legacy_cpu, cpu);
    assert_eq!(legacy_bus.events, bus.events);
    assert_eq!(legacy_bus.words, bus.words);
    assert_eq!(legacy_bus.clock, bus.clock);
    assert_eq!(bus.clock, 57);
    assert_eq!(cpu.clock().0, 57);
    for at in 0..5 {
        for failure in [Failure::Host, Failure::Boundary, Failure::Unavailable] {
            capture::failed(case, at, failure);
        }
    }
}

#[test]
fn nine_literal_raw_first_odd_frames_include_retained_cmpa_processing_class() {
    use Family::{AbsoluteWord as AW, Displacement as D, Indexed as I, Indirect as R, Post, Pre};
    use Size::{Long as L, Word as W};
    let rows = [
        (
            W,
            R,
            1,
            0x0067_cb0c_u32,
            0x3d0a_57e7_u32,
            0x2210,
            1,
            [0xcb12, 0x2210, 0x0067, 0x32bc, 0x57e7, 0x32a5, 0x3d0a],
            62,
        ),
        (
            W,
            Post,
            3,
            0x001e_17e2,
            0xd648_768d,
            0x0200,
            1,
            [0x17e8, 0x0200, 0x001e, 0x36fc, 0x768d, 0x36e1, 0xd648],
            62,
        ),
        (
            W,
            Pre,
            1,
            0x009f_5c22,
            0xc02e_58a7,
            0x2100,
            1,
            [0x5c28, 0x2100, 0x009f, 0xb9fc, 0x58a7, 0xb9e5, 0xc02e],
            66,
        ),
        (
            W,
            I,
            4,
            0x004a_517c,
            0x80da_9651,
            0x2618,
            0x8000,
            [0x5182, 0x2618, 0x004a, 0x39bc, 0x9651, 0x39a5, 0x80da],
            68,
        ),
        (
            W,
            AW,
            0,
            0x00a3_35c0,
            0xffff_fd19,
            0x0700,
            1,
            [0x35c6, 0x0700, 0x00a3, 0x31fc, 0xfd19, 0x31e1, 0xffff],
            66,
        ),
        (
            L,
            R,
            1,
            0x000c_6de8,
            0x22b8_1fe7,
            0x2511,
            0x8000_0001,
            [0x6df0, 0x2511, 0x000c, 0x22bc, 0x1fe7, 0x22a5, 0x22b8],
            66,
        ),
        (
            L,
            Post,
            4,
            0x0098_9660,
            0xd9ec_4719,
            0x041d,
            0x8000_0001,
            [0x9668, 0x041d, 0x0098, 0x28fc, 0x4719, 0x28e1, 0xd9ec],
            66,
        ),
        (
            L,
            Pre,
            4,
            0x0089_a09a,
            0x33d8_5671,
            0x0610,
            0x0001_0000,
            [0xa0a2, 0x0610, 0x0089, 0x293c, 0x5671, 0x2921, 0x33d8],
            70,
        ),
        (
            L,
            D,
            5,
            0x007e_60f2,
            0xad0f_7083,
            0x0312,
            0x0001_0000,
            [0x60fa, 0x0312, 0x007e, 0x2b7c, 0x7083, 0x2b61, 0xad0f],
            70,
        ),
    ];
    let mut frames = 0;
    for (size, family, destination, pc, ea, sr, value, words, clocks) in rows {
        let form = Form { size, family };
        let mut case = Case::new(form);
        case.r = registers(sr & SUPERVISOR != 0, 0);
        case.r.set_status_register(sr);
        case.r.pc = pc;
        case.destination = destination;
        case.value = value;
        case.low = value as u16;
        let base = match family {
            R | Post => Some(ea),
            Pre => {
                if size == W {
                    case.next = words[3];
                }
                Some(ea.wrapping_add(2))
            }
            D => {
                case.extension = 1;
                Some(ea.wrapping_sub(1))
            }
            I => {
                case.extension = 0x4801;
                case.r.d[4] = 0;
                Some(ea.wrapping_sub(1))
            }
            AW => {
                case.extension = ea as u16;
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
        assert_eq!(
            [
                c.internal_pc as u16,
                c.phase_sr,
                (c.internal_pc >> 16) as u16,
                c.ird,
                c.logical_address as u16,
                (c.ird & 0xffe0) | c.function_code as u16,
                (c.logical_address >> 16) as u16
            ],
            words
        );
        bus.detection = Some((Failure::Boundary, 8));
        let result = cpu.step_physical_instruction(&mut bus);
        frames += 1;
        assert_eq!(
            result,
            Ok(PhysicalInstructionStep::Group0 {
                opcode: form.opcode(destination),
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
        assert_eq!(bus.count, at);
        assert_eq!(cpu.clock().0, bus.clock);
    }
    assert_eq!(frames, 9);
}
