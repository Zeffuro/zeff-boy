use zeff_m68k::bus::Direction;
use zeff_m68k::timing::TimingFault;
use zeff_md_core::{BusError, Emulator, LoadConfig, MachineFault, Region};

fn image(ssp: u32, words: &[u16]) -> Vec<u8> {
    let mut bytes = vec![0; 0x400];
    bytes[..4].copy_from_slice(&ssp.to_be_bytes());
    bytes[4..8].copy_from_slice(&0x200_u32.to_be_bytes());
    for (index, value) in words.iter().enumerate() {
        bytes[0x200 + index * 2..0x202 + index * 2].copy_from_slice(&value.to_be_bytes());
    }
    bytes
}

#[cfg_attr(test, test)]
pub fn unused_reset_stack_is_preserved_until_guest_replaces_it() {
    let program = [
        0x4e71, 0x2e7c, 0x00ff, 0xfff0, 0x2f3c, 0x1122, 0x3344, 0x201f, 0x13fc, 0x0085, 0x00c0,
        0x0011, 0x13fc, 0x0006, 0x00c0, 0x0011, 0x13fc, 0x0090, 0x00c0, 0x0011, 0x60fe, 0x4e71,
    ];
    for region in [Region::NtscUs, Region::NtscJapan, Region::Pal] {
        for ssp in [0x200, 0x00ff_fffd, 0x4e75_cb92, 0xdead_beef] {
            let rom = image(ssp, &program);
            let mut actual = Emulator::new(
                rom.clone(),
                LoadConfig {
                    region,
                    concurrent_sound: true,
                },
            )
            .unwrap();
            actual.set_sample_rate(48_000);
            assert_eq!(actual.registers().ssp, ssp);
            let raw = actual.encode_state().unwrap();
            let frontend = actual.encode_frontend_state().unwrap();
            let mut raw_restored = Emulator::new(
                rom.clone(),
                LoadConfig {
                    region,
                    concurrent_sound: true,
                },
            )
            .unwrap();
            raw_restored.set_sample_rate(48_000);
            raw_restored.decode_state(&raw).unwrap();
            assert_eq!(raw_restored.registers().ssp, ssp);
            let mut restored = Emulator::new(
                rom.clone(),
                LoadConfig {
                    region,
                    concurrent_sound: true,
                },
            )
            .unwrap();
            restored.set_sample_rate(48_000);
            restored.decode_frontend_state(&frontend).unwrap();
            assert_eq!(restored.registers().ssp, ssp);
            let mut restarted = actual.clone();
            restarted.step_instruction().unwrap();
            restarted.reset_cpu().unwrap();
            assert_eq!(restarted.registers().ssp, ssp);
            assert_eq!(restarted.registers().pc, 0x200);
            restarted.step_instruction().unwrap();
            assert_eq!(restarted.registers().ssp, ssp);
            let mut cloned = actual.clone();
            for step in 0..4 {
                let expected = actual.step_instruction().unwrap();
                assert_eq!(restored.step_instruction().unwrap(), expected);
                assert_eq!(raw_restored.step_instruction().unwrap(), expected);
                assert_eq!(cloned.step_instruction().unwrap(), expected);
                assert_eq!(actual, restored);
                assert_eq!(actual, cloned);
                assert_eq!(
                    actual.encode_state().unwrap(),
                    raw_restored.encode_state().unwrap()
                );
                assert_eq!(
                    actual.registers().ssp,
                    if step == 0 {
                        ssp
                    } else if step == 2 {
                        0x00ff_ffec
                    } else {
                        0x00ff_fff0
                    }
                );
            }
            assert_eq!(actual.registers().d[0], 0x1122_3344);
            assert_eq!(
                (actual.peek(0xffffec), actual.peek(0xffffef)),
                (Some(0x11), Some(0x44))
            );
            for _ in 0..2 {
                actual.step_field().unwrap();
                restored.step_field().unwrap();
                cloned.step_field().unwrap();
                raw_restored.step_field().unwrap();
                assert_eq!(
                    actual.encode_state().unwrap(),
                    restored.encode_state().unwrap()
                );
                assert_eq!(
                    actual.encode_state().unwrap(),
                    cloned.encode_state().unwrap()
                );
                assert_eq!(
                    actual.encode_state().unwrap(),
                    raw_restored.encode_state().unwrap()
                );
                assert_eq!(actual.framebuffer(), restored.framebuffer());
                assert_eq!(actual.framebuffer(), cloned.framebuffer());
                let pcm = actual.drain_audio_samples();
                assert!(!pcm.is_empty() && pcm.iter().any(|sample| *sample != 0.0));
                assert_eq!(pcm, restored.drain_audio_samples());
                assert_eq!(pcm, cloned.drain_audio_samples());
                assert_eq!(pcm, raw_restored.drain_audio_samples());
            }
        }
    }
}

#[cfg_attr(test, test)]
pub fn reset_admission_does_not_make_stack_accesses_valid() {
    let program = [0x4e71, 0x2f00, 0x60fe, 0x4e71];
    let mut unused = Emulator::new(image(0x0040_0006, &program), LoadConfig::default()).unwrap();
    assert_eq!(unused.registers().ssp, 0x0040_0006);
    unused.step_instruction().unwrap();
    assert_eq!(unused.registers().ssp, 0x0040_0006);
    assert_eq!(unused.step_instruction().unwrap().elapsed.get(), 84);
    assert_eq!(unused.registers().ssp, 0x0040_0002);
    for address in 0x0040_0002..0x0040_0006 {
        assert_eq!(unused.peek(address), None);
    }
    unused.step_instruction().unwrap();

    let mut actual = Emulator::new(image(0x0080_0006, &program), LoadConfig::default()).unwrap();
    assert_eq!(actual.registers().ssp, 0x0080_0006);
    actual.step_instruction().unwrap();
    assert_eq!(actual.registers().ssp, 0x0080_0006);
    let fault = actual.step_instruction().unwrap_err();
    let MachineFault::Main(TimingFault::Bus {
        access,
        direction: Direction::Write,
        source:
            BusError::UnsupportedAccess {
                address,
                direction: Direction::Write,
            },
    }) = fault
    else {
        panic!("unexpected stack access fault: {fault:?}");
    };
    assert_eq!(access.bus_address, 0x0080_0004);
    assert_eq!(address, access.bus_address);
    let mut odd = Emulator::new(image(0x00ff_fffd, &program), LoadConfig::default()).unwrap();
    odd.step_instruction().unwrap();
    assert_eq!(odd.registers().ssp, 0x00ff_fffd);
    assert_eq!(
        odd.step_instruction(),
        Err(MachineFault::Main(TimingFault::AddressError {
            address: 0x00ff_fffb
        }))
    );
}
