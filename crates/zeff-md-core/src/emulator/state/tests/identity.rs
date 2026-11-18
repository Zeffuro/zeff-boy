use super::*;
use crate::cartridge::Cartridge;
use sha2::{Digest, Sha256};
use std::sync::Arc;

fn digest(cartridge: &Cartridge) -> [u8; 32] {
    Sha256::digest(cartridge.bytes.as_ref()).into()
}

fn raw_with_current_digest(machine: &Emulator) -> Vec<u8> {
    let expected = digest(&machine.bus.cartridge);
    let saved = machine.encode_state().unwrap();
    assert_eq!(&saved[12..44], expected.as_slice());
    assert_eq!(machine.bus.cartridge.rom_digest(), expected);
    assert_eq!(
        u32::from_le_bytes(saved[44..48].try_into().unwrap()) as usize,
        machine.rom_bytes().len()
    );
    assert_eq!(
        u32::from_le_bytes(saved[8..12].try_into().unwrap()),
        STATE_VERSION
    );
    saved
}

fn wrong_rom_is_atomic(target: &mut Emulator, raw: &[u8], frontend: &[u8]) {
    let before = target.clone();
    for frontend_state in [false, true] {
        let error = if frontend_state {
            target.decode_frontend_state(frontend).unwrap_err()
        } else {
            target.decode_state(raw).unwrap_err()
        };
        assert!(error.to_string().contains("ROM mismatch"));
        assert_eq!(*target, before);
    }
}

fn digest_boundaries() {
    for warm in [false, true] {
        for change in 0..4 {
            let source = machine(Region::NtscUs, false);
            let original = Arc::clone(&source.bus.cartridge.bytes);
            let original_digest = digest(&source.bus.cartridge);
            if warm {
                assert_eq!(source.bus.cartridge.rom_digest(), original_digest);
            }
            let mut detached = source.clone();
            match change {
                0 => detached.bus.cartridge.bytes = Arc::from(original.to_vec()),
                1 | 2 => {
                    let mut bytes = original.to_vec();
                    if change == 1 {
                        *bytes.last_mut().unwrap() ^= 1;
                    } else {
                        bytes.push(0x77);
                    }
                    detached.bus.cartridge.bytes = bytes.into();
                }
                3 => {
                    *Arc::make_mut(&mut detached.bus.cartridge.bytes)
                        .last_mut()
                        .unwrap() ^= 1;
                }
                _ => unreachable!(),
            }
            assert!(!Arc::ptr_eq(&original, &detached.bus.cartridge.bytes));
            let detached_raw = raw_with_current_digest(&detached);
            if !warm && change == 3 {
                let mut returned = detached.clone();
                returned.bus.cartridge.bytes = Arc::clone(&original);
                assert_eq!(returned.bus.cartridge.rom_digest(), original_digest);
            }
            assert_eq!(source.bus.cartridge.rom_digest(), original_digest);
            let raw = raw_with_current_digest(&source);
            let frontend = source.encode_frontend_state().unwrap();
            if change == 0 {
                assert_eq!(detached_raw, raw);
                detached.decode_state(&raw).unwrap();
                detached.decode_frontend_state(&frontend).unwrap();
                assert_eq!(detached.bus.cartridge, source.bus.cartridge);
            } else {
                assert_ne!(detached_raw[12..44], raw[12..44]);
                wrong_rom_is_atomic(&mut detached, &raw, &frontend);
                assert_eq!(raw_with_current_digest(&detached), detached_raw);
            }
            detached.bus.cartridge.bytes = Arc::clone(&original);
            assert_eq!(raw_with_current_digest(&detached), raw);
            assert_eq!(detached.bus.cartridge, source.bus.cartridge);
        }
    }

    let mut source = machine(Region::NtscUs, false);
    assert!(Arc::get_mut(&mut source.bus.cartridge.bytes).is_none());
    let peer = Cartridge::new(source.rom_bytes().to_vec()).unwrap();
    let cartridge = &source.bus.cartridge;
    let expected_debug = format!(
        concat!(
            "Cartridge {{ bytes: {:?}, initial: {:?}, header: {:?}, save: {:?}, parallel_adapter: {:?}, ",
            "registers: {:?}, protection: {:?}, sf001: {:?}, sf002: {:?}, sf004: {:?}, extended_ssf: {:?}, ssf2: {:?}, ",
            "lock_on: {:?}, lock_on_save: {:?} }}"
        ),
        cartridge.bytes,
        cartridge.initial,
        cartridge.header,
        cartridge.save,
        cartridge.parallel_adapter,
        cartridge.registers,
        cartridge.protection,
        cartridge.sf001,
        cartridge.sf002,
        cartridge.sf004,
        cartridge.extended_ssf,
        cartridge.ssf2,
        cartridge.lock_on,
        cartridge.lock_on_save
    );
    assert_eq!(format!("{cartridge:?}"), expected_debug);
    let before = source.clone();
    let original = Arc::clone(&source.bus.cartridge.bytes);
    let expected = digest(&source.bus.cartridge);
    assert_eq!(before.bus.cartridge.rom_digest(), expected);
    assert_eq!(source.bus.cartridge, peer);
    assert_eq!(format!("{:?}", source.bus.cartridge), format!("{peer:?}"));
    assert_eq!(source, before);
    source.reset();
    assert!(Arc::ptr_eq(&source.bus.cartridge.bytes, &original));
    assert_eq!(source.bus.cartridge.rom_digest(), expected);
    let _ = raw_with_current_digest(&source);
    crate::cartridge::assert_digest_memo_sharing(original.as_ref());

    #[cfg(not(target_arch = "wasm32"))]
    concurrent_first_digest(original.as_ref());
}

#[cfg(not(target_arch = "wasm32"))]
fn concurrent_first_digest(bytes: &[u8]) {
    let source = Cartridge::new(bytes.to_vec()).unwrap();
    let expected = digest(&source);
    let barrier = Arc::new(std::sync::Barrier::new(5));
    let workers: Vec<_> = (0..4)
        .map(|index| {
            let mut cartridge = source.clone();
            if index == 3 {
                *Arc::make_mut(&mut cartridge.bytes).last_mut().unwrap() ^= 1;
            }
            let expected = digest(&cartridge);
            let barrier = Arc::clone(&barrier);
            std::thread::spawn(move || {
                barrier.wait();
                assert_eq!(cartridge.rom_digest(), expected);
                assert_eq!(cartridge.clone().rom_digest(), expected);
            })
        })
        .collect();
    barrier.wait();
    assert_eq!(source.rom_digest(), expected);
    for worker in workers {
        worker.join().unwrap();
    }
}

fn late_first_use_is_atomic() {
    let source = active(Region::NtscUs, false);
    let raw = source.encode_state().unwrap();
    let frontend = source.encode_frontend_state().unwrap();
    for frontend_state in [false, true] {
        let mut target = active(Region::NtscUs, false);
        let mut control = target.clone();
        if frontend_state {
            let mut bad = frontend.clone();
            let mut reader = StateReader::new(&bad);
            reader.set_position(12);
            reader.read_slice(MAX_STATE_BYTES).unwrap();
            let audio_at = reader.position() + 4;
            bad[audio_at + 599] = 34;
            assert!(target.decode_frontend_state(&bad).is_err());
        } else {
            let mut bad = raw.clone();
            *bad.last_mut().unwrap() = 2;
            assert!(target.decode_state(&bad).is_err());
        }
        assert_eq!(target, control);
        assert_eq!(
            target.encode_state().unwrap(),
            control.encode_state().unwrap()
        );
        assert_eq!(
            target.encode_frontend_state().unwrap(),
            control.encode_frontend_state().unwrap()
        );
        for _ in 0..1021 {
            assert_eq!(target.step_instruction(), control.step_instruction());
        }
        assert_eq!(target.framebuffer(), control.framebuffer());
        assert_eq!(
            target.encode_state().unwrap(),
            control.encode_state().unwrap()
        );
        let samples = control.drain_audio_samples();
        assert!(samples.iter().any(|sample| *sample != 0.0));
        assert_eq!(target.drain_audio_samples(), samples);
    }
    let cartridge = crate::emulator::sf002_tests::cartridge();
    let mut injected = Emulator::new(cartridge.bytes.to_vec(), LoadConfig::default()).unwrap();
    injected.bus.cartridge = cartridge;
    assert_eq!(
        injected.bus.cartridge.rom_digest(),
        digest(&injected.bus.cartridge)
    );
    let before = injected.clone();
    assert!(
        injected
            .encode_state()
            .unwrap_err()
            .to_string()
            .contains("SF-002 identity mismatch")
    );
    assert_eq!(injected, before);
}

pub(super) fn preserves_live_machine() {
    digest_boundaries();
    late_first_use_is_atomic();
    let source = active(Region::NtscUs, false);
    let saved = source.encode_state().unwrap();
    let mut target = source.clone();
    for _ in 0..23 {
        target.step_instruction().unwrap();
    }
    let before = target.clone();
    let mut rejects = vec![
        saved[..12].to_vec(),
        saved[..52].to_vec(),
        vec![0; MAX_STATE_BYTES + 1],
    ];
    for length in saved.len() - 24..saved.len() {
        rejects.push(saved[..length].to_vec());
    }
    for (offset, value) in [
        (0, b'X'),
        (8, 0),
        (8, (STATE_VERSION + 1) as u8),
        (12, saved[12] ^ 1),
        (48, 1),
        (49, 0),
        (50, 1),
        (51, 1),
        (saved.len() - 1, 2),
    ] {
        let mut changed = saved.clone();
        changed[offset] = value;
        rejects.push(changed);
    }
    let mut extra = saved.clone();
    extra.push(0);
    rejects.push(extra);
    for bad in rejects {
        assert!(target.decode_state(&bad).is_err());
        assert_eq!(target, before);
    }
    let pal = active(Region::Pal, false);
    assert!(target.decode_state(&pal.encode_state().unwrap()).is_err());
    assert_eq!(target, before);
    let serial = machine(Region::NtscUs, false);
    assert!(
        target
            .decode_state(&serial.encode_state().unwrap())
            .is_err()
    );
    assert_eq!(target, before);
}
