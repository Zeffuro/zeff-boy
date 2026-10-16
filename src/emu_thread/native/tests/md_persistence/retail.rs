use super::*;
use serde::Deserialize;
use serde_json::json;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Case {
    rom: PathBuf,
    source: PathBuf,
    path: PathBuf,
    output: PathBuf,
    japan: bool,
    player_fields: usize,
    cold_fields: usize,
    player_inputs: Vec<(usize, u8)>,
    cold_inputs: Vec<(usize, u8)>,
    expected_save_sha256: String,
    expected_player_frame_sha256: String,
    expected_cold_frame_sha256: String,
}

fn run(backend: &mut EmuBackend, fields: usize, events: &[(usize, u8)]) {
    for field in 0..fields {
        let raw = events
            .iter()
            .rev()
            .find(|(at, _)| *at <= field)
            .map_or(0xff, |(_, raw)| *raw);
        let pressed = !raw;
        let buttons = (pressed & 0x40)
            | (pressed >> 4 & 1)
            | ((pressed >> 5 & 1) << 1)
            | ((pressed >> 7 & 1) << 3);
        let dpad = (pressed >> 3 & 1)
            | ((pressed >> 2 & 1) << 1)
            | ((pressed & 1) << 2)
            | ((pressed >> 1 & 1) << 3);
        backend.set_input(buttons, dpad);
        execute(backend, 1);
    }
}

fn hash(bytes: &[u8]) -> String {
    const_hex::encode(zeff_firmware::sha256_bytes(bytes))
}

#[test]
#[ignore = "opt-in selected MD game saves through isolated native application terminal worker"]
fn md_application_save_terminal_diagnostic() {
    let manifest = std::env::var_os("ZEFF_MD_SAVE_CASES").expect("set ZEFF_MD_SAVE_CASES");
    let cases: Vec<Case> = serde_json::from_slice(&std::fs::read(manifest).unwrap()).unwrap();
    for case in cases {
        assert!(case.output.is_dir() && !case.source.with_extension("sav").exists());
        let owned = case.output.canonicalize().unwrap();
        assert!(case.source.canonicalize().unwrap().starts_with(&owned));
        assert!(
            case.path == case.source
                || case.path.strip_prefix(&case.source).is_ok_and(|part| part
                    .components()
                    .all(|c| matches!(c, std::path::Component::Normal(_))))
        );
        for name in ["generation.bin", "recovery.mdstate", "result.json"] {
            assert!(!case.output.join(name).exists());
        }
        let rom = std::fs::read(&case.rom).unwrap();
        let region = if case.japan {
            Region::NtscJapan
        } else {
            Region::NtscUs
        };
        let mut backend = load(&rom, &case.source, &case.path, region);
        run(&mut backend, case.player_fields, &case.player_inputs);
        let expected = cells(&backend);
        let expected_state = backend.encode_state_bytes().unwrap();
        assert_eq!(hash(&expected), case.expected_save_sha256);
        assert_eq!(
            hash(backend.framebuffer()),
            case.expected_player_frame_sha256
        );
        let generation = case.output.join("generation.bin");
        let recovery = case.output.join("recovery.mdstate");
        let responses = terminal(backend, generation.clone(), recovery.clone(), false);
        assert!(matches!(
            &responses[..],
            [
                EmuResponse::SramFlushed(Some(_)),
                EmuResponse::RecoverySaved(_),
                EmuResponse::ShutdownComplete
            ]
        ));
        let save = case.source.with_extension("sav");
        let mut verify = load(&rom, &case.source, &case.path, region);
        verify_files(
            &mut verify,
            &save,
            &generation,
            &recovery,
            &expected,
            &expected_state,
        );
        drop(verify);
        let mut cold = load(&rom, &case.source, &case.path, region);
        assert_eq!(cells(&cold), expected);
        run(&mut cold, case.cold_fields, &case.cold_inputs);
        assert_eq!(cells(&cold), expected);
        assert_eq!(hash(cold.framebuffer()), case.expected_cold_frame_sha256);
        std::fs::write(case.output.join("result.json"), serde_json::to_vec_pretty(&json!({
            "status":"PASS_NATIVE_APPLICATION_GUEST_TERMINAL_COLD_LOAD", "fields":[case.player_fields,case.cold_fields],
            "rom_sha256":hash(&rom),"save_bytes":expected.len(),"save_sha256":hash(&expected),
            "cold_frame_sha256":hash(cold.framebuffer()),"generation_sha256":hash(&std::fs::read(generation).unwrap()),
            "recovery_sha256":hash(&std::fs::read(recovery).unwrap()),"cold_used_state_restore":false
        })).unwrap()).unwrap();
    }
}
