use super::*;

#[test]
fn active_batches_preserve_busy_expiry_timer_clear_and_banked_reservation_replay() {
    for case in 0..3 {
        let (mut cartridge, mut cpu, mut memory, mut audio, mut schedule) =
            setup(if case == 2 { 0xc000 } else { 0x4000 });
        if case == 2 {
            memory.bank = 0x140;
        }
        for (register, value) in if case == 1 {
            vec![(0x24, 0xff), (0x25, 3), (0x27, 5)]
        } else {
            vec![(0x2a, 0x90)]
        } {
            audio.write_opn2(0, 0, register);
            audio.write_opn2(0, 1, value);
        }
        let (start, split, end) = match case {
            0 => (1185, 1336, 1380),
            1 => (1134, 1286, 1326),
            _ => (0, 151, 1545),
        };
        audio.advance_to(start);
        let mut reference = (cpu.clone(), memory.clone(), audio.clone(), schedule.clone());
        audio.begin_batch();
        for (begin, at) in [(start, split), (split, end)] {
            let occupied: &[Reservation] = if case == 2 && begin == split {
                &[Reservation {
                    requester: Requester::MAIN,
                    ticks: 150..1500,
                }]
            } else {
                &[]
            };
            schedule.advance(
                &mut cpu,
                &mut memory,
                &mut cartridge,
                &mut [],
                SoundInterval {
                    ticks: begin..at,
                    occupied,
                    audio: &mut audio,
                    sound_irq: None,
                },
            );
            reference.3.advance(
                &mut reference.0,
                &mut reference.1,
                &mut cartridge,
                &mut [],
                SoundInterval {
                    ticks: begin..at,
                    occupied,
                    audio: &mut reference.2,
                    sound_irq: None,
                },
            );
            if case == 1 && at == split {
                for audio in [&mut audio, &mut reference.2] {
                    audio.write_opn2(at, 0, 0x27);
                    audio.write_opn2(at, 1, 0x10);
                }
            }
        }
        audio.finish_batch();
        assert_eq!(schedule.fault, None);
        assert_eq!(cpu.regs().a, if case == 1 { 0x80 } else { 0 });
        assert_eq!(cpu, reference.0);
        assert_eq!(memory, reference.1);
        assert!(audio == reference.2, "full audio case={case}");
        assert_eq!(schedule, reference.3);
    }
}
