use super::*;

#[test]
fn register_interruption_restores_pending_and_completed_fetches() {
    for region in [Region::NtscUs, Region::NtscJapan, Region::Pal] {
        for h40 in [false, true] {
            for display in [false, true] {
                let mut seed = Vdp::bootstrap_mode5();
                register(&mut seed, AUTO_INCREMENT_REGISTER, 2, region);
                register(
                    &mut seed,
                    DISPLAY_SIZE_REGISTER,
                    if h40 { 0x81 } else { 0 },
                    region,
                );
                register(&mut seed, 5, 0, region);
                register(
                    &mut seed,
                    MODE_REGISTER,
                    MODE5_ENABLE | if display { DISPLAY_ENABLE } else { 0 },
                    region,
                );
                seed.invalidate_interval();
                for (address, data) in [(0x3ffe, 0x1357), (0x240, 0x2468)] {
                    seed.memory
                        .write_word(
                            ControlCommand {
                                address,
                                code: VRAM_WRITE_CODE,
                            },
                            data,
                        )
                        .unwrap();
                }
                command(&mut seed, 0x3ffe, VRAM_READ_CODE, region);
                for slots in 0..=2 {
                    let mut source = seed.clone();
                    for _ in 0..slots {
                        let at = next_slot(&source, region);
                        source.advance_to(at, region).unwrap();
                    }
                    assert_eq!(
                        source.state.read_phase,
                        match slots {
                            0 => Some(false),
                            1 => Some(true),
                            _ => None,
                        }
                    );
                    let mut target = restored(&source, region);
                    let buffered = source.read_cache().buffered_word();
                    for vdp in [&mut source, &mut target] {
                        vdp.write_control(0x8f02, region).unwrap();
                        assert_eq!(vdp.state.read_phase, None);
                        assert!(!vdp.read_cache().is_ready());
                        assert_eq!(vdp.read_cache().buffered_word(), buffered);
                    }
                    assert_eq!(source, target);
                    target = restored(&source, region);
                    let at = tick(source.now().get() + 3420);
                    source.advance_to(at, region).unwrap();
                    target.advance_to(at, region).unwrap();
                    assert_eq!(source, target);
                    let address = source.command().address;
                    assert_eq!(address & 0xc000, if slots == 2 { 0x4000 } else { 0 });
                    for vdp in [&mut source, &mut target] {
                        command(vdp, 0x240, VRAM_READ_CODE, region);
                        let first = next_slot(vdp, region);
                        vdp.advance_to(first, region).unwrap();
                        assert_eq!(vdp.state.read_phase, Some(true));
                        assert!(!vdp.read_cache().is_ready());
                    }
                    assert_eq!(source, target);
                    target = restored(&source, region);
                    let second = next_slot(&source, region);
                    source.advance_to(second, region).unwrap();
                    target.advance_to(second, region).unwrap();
                    assert_eq!(source, target);
                    assert_eq!(source.read_data(region), Ok(0x2468));
                    assert_eq!(target.read_data(region), Ok(0x2468));
                    assert_eq!(source, target);
                    restored(&source, region);
                }
            }
        }
    }
}
