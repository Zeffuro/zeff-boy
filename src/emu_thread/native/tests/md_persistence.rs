use super::super::*;
use crate::emu_backend::{ActiveSystem, BackendLoadConfig, load_backend_from_rom_source};
use crate::emu_thread::RecoveryTestConfig;
use crate::save_paths::recovery_state::{
    BatteryGenerationWitness, RecoveryStateIdentity, decode_battery_generation,
    decode_recovery_state,
};
use std::path::{Path, PathBuf};
use zeff_md_core::{ControllerKind, Region};

mod retail;

fn load(bytes: &[u8], source: &Path, path: &Path, region: Region) -> EmuBackend {
    load_backend_from_rom_source(
        ActiveSystem::Md,
        source,
        path,
        Some(bytes.to_vec()),
        BackendLoadConfig {
            md_region: Some(region),
            md_controller: ControllerKind::ThreeButton,
            sample_rate: Some(44_100),
            ..Default::default()
        },
    )
    .unwrap()
    .backend
}

fn cells(backend: &EmuBackend) -> Vec<u8> {
    let EmuBackend::Md(md) = backend else {
        panic!("MD backend expected");
    };
    md.emu.dump_save_ram().unwrap()
}

fn terminal(
    backend: EmuBackend,
    generation: PathBuf,
    recovery: PathBuf,
    fail_generation_write: bool,
) -> Vec<EmuResponse> {
    let mut worker = EmuThread::try_spawn_inner(
        backend,
        true,
        None,
        Some(RecoveryTestConfig {
            generation_path: generation,
            state_path: recovery,
            fail_generation_write,
        }),
    )
    .unwrap();
    assert!(worker.send_checked(EmuCommand::Shutdown));
    let mut responses = Vec::new();
    loop {
        let response = worker
            .resp_rx
            .recv_timeout(Duration::from_secs(10))
            .unwrap();
        let done = matches!(response, EmuResponse::ShutdownComplete);
        responses.push(response);
        if done {
            break;
        }
    }
    worker.join.take().unwrap().join().unwrap();
    assert!(worker.resp_rx.try_recv().is_err());
    responses
}

fn verify_files(
    backend: &mut EmuBackend,
    save: &Path,
    generation: &Path,
    recovery: &Path,
    expected: &[u8],
    expected_state: &[u8],
) {
    assert_eq!(std::fs::read(save).unwrap(), expected);
    let receipt = backend.battery_generation_receipt().unwrap();
    assert!(receipt.is_consistent());
    assert_eq!(receipt.components.len(), 1);
    assert_eq!(receipt.components[0].byte_len, expected.len() as u64);
    assert_eq!(
        receipt.components[0].sha256,
        zeff_firmware::sha256_bytes(expected)
    );
    let identity = backend.rom_hash();
    let record = decode_battery_generation(&std::fs::read(generation).unwrap(), identity).unwrap();
    assert_eq!(record.component_sha256, receipt.component_sha256);
    let discriminator = backend.recovery_discriminator();
    let envelope = decode_recovery_state(
        &std::fs::read(recovery).unwrap(),
        RecoveryStateIdentity {
            system: "md",
            discriminator: &discriminator,
            media_sha256: identity,
        },
    )
    .unwrap();
    assert_eq!(envelope.native_payload, expected_state);
    assert_eq!(
        envelope.battery,
        BatteryGenerationWitness::Committed {
            generation: record.generation,
            component_sha256: record.component_sha256,
        }
    );
    backend
        .load_state_from_bytes(envelope.native_payload.clone())
        .unwrap();
    assert_eq!(
        backend.encode_state_bytes().unwrap(),
        envelope.native_payload
    );
    assert_eq!(cells(backend), expected);
}

fn guest_rom(size: usize) -> Vec<u8> {
    let mut bytes = vec![0; 0x400];
    bytes[..8].copy_from_slice(&[0, 0xff, 0xff, 0xfc, 0, 0, 2, 0]);
    bytes[0x1b0..0x1ba].copy_from_slice(&[b'R', b'A', 0xf8, 0x20, 0, 0x20, 0, 1, 0, 0x20]);
    bytes[0x1ba..0x1bc].copy_from_slice(&((size * 2 - 1) as u16).to_be_bytes());
    let words = [
        0x13fc,
        1,
        0x00a1,
        0x30f1,
        0x207c,
        0x0020,
        1,
        0x7000,
        0x3e3c,
        (size - 1) as u16,
        0x3200,
        0xe049,
        0xb101,
        0x0601,
        0x37,
        0x1081,
        0x5488,
        0x5240,
        0x51cf,
        0xffee,
        0x60fe,
        0x4e71,
    ];
    for (index, word) in words.into_iter().enumerate() {
        bytes[0x200 + index * 2..0x202 + index * 2].copy_from_slice(&word.to_be_bytes());
    }
    bytes
}

fn execute(backend: &mut EmuBackend, frames: usize) {
    let mut audio = Vec::new();
    for _ in 0..frames {
        backend.step_frame();
        assert!(backend.take_runtime_fault().is_none());
        backend.drain_audio_samples_into(&mut audio);
        audio.clear();
    }
}

#[test]
fn md_guest_saves_commit_through_native_worker_shutdown_and_cold_backend() {
    for size in [512, 8192, 32768] {
        for archive in [false, true] {
            let root = crate::test_support::test_directory("md-native-terminal").unwrap();
            let source = root
                .path()
                .join(if archive { "owned.zip" } else { "owned.md" });
            let path = if archive {
                source.join("folder/game.md")
            } else {
                source.clone()
            };
            let save = source.with_extension("sav");
            let generation = root.path().join("generation.bin");
            let recovery = root.path().join("recovery.mdstate");
            let rom = guest_rom(size);
            let mut backend = load(&rom, &source, &path, Region::NtscUs);
            execute(&mut backend, 64);
            let expected: Vec<_> = (0..size)
                .map(|i| ((i as u8) ^ (i >> 8) as u8).wrapping_add(0x37))
                .collect();
            assert_eq!(cells(&backend), expected);
            let expected_state = backend.encode_state_bytes().unwrap();
            let responses = terminal(backend, generation.clone(), recovery.clone(), false);
            assert!(
                matches!(&responses[..], [EmuResponse::SramFlushed(Some(p)), EmuResponse::RecoverySaved(r), EmuResponse::ShutdownComplete] if p == &save.display().to_string() && r == &recovery)
            );
            let mut cold = load(&rom, &source, &path, Region::NtscUs);
            assert_eq!(cells(&cold), expected);
            verify_files(
                &mut cold,
                &save,
                &generation,
                &recovery,
                &expected,
                &expected_state,
            );
        }
    }
}

#[test]
fn md_native_failed_terminal_barriers_never_acknowledge_success() {
    for size in [8192, 32768] {
        for fail_generation in [false, true] {
            let root = crate::test_support::test_directory("md-native-terminal-failure").unwrap();
            let source = root.path().join("owned.md");
            let save = source.with_extension("sav");
            let generation = root.path().join("generation.bin");
            let recovery = root.path().join("recovery.mdstate");
            let mut backend = load(&guest_rom(size), &source, &source, Region::NtscUs);
            execute(&mut backend, 64);
            let expected = cells(&backend);
            std::fs::write(&generation, b"prior-generation").unwrap();
            std::fs::write(&recovery, b"prior-recovery").unwrap();
            if !fail_generation {
                std::fs::create_dir(&save).unwrap();
                std::fs::write(save.join("preserved"), b"keep").unwrap();
            }
            let responses = terminal(
                backend,
                generation.clone(),
                recovery.clone(),
                fail_generation,
            );
            assert!(matches!(
                &responses[..],
                [
                    EmuResponse::SramFlushFailed(_),
                    EmuResponse::RecoverySaveFailed(_),
                    EmuResponse::ShutdownComplete
                ]
            ));
            assert_eq!(std::fs::read(generation).unwrap(), b"prior-generation");
            assert_eq!(std::fs::read(recovery).unwrap(), b"prior-recovery");
            if fail_generation {
                assert_eq!(std::fs::read(save).unwrap(), expected);
            } else {
                assert_eq!(std::fs::read(save.join("preserved")).unwrap(), b"keep");
            }
        }
    }
}
