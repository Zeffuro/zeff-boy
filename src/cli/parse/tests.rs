use super::input::parse_zapper_event_arg;
use super::parse_args_from;
use super::values::{
    parse_pce_arcade_card_mode_arg, parse_pce_controller_mode_arg, parse_pce_memory_base_mode_arg,
    parse_region_dump_arg, parse_sega8_console_region_arg, parse_sega8_video_standard_arg,
};
use zeff_gb_core::hardware::types::hardware_mode::HardwareModePreference;
use zeff_pce_core::hardware::{PceArcadeCardMode, PceControllerMode, PceMemoryBaseMode};
use zeff_sega8_core::hardware::region::Sega8Region;
use zeff_sega8_core::hardware::timing::Sega8VideoStandard;

#[test]
fn parses_headless_tas_project_verification_options() {
    let args = parse_args_from([
        "--headless",
        "--tas-verify",
        "movie.ztas",
        "--tas-branch",
        "route-b",
        "--tas-export",
        "verified.zrpl",
        "game.nes",
    ])
    .unwrap();

    let headless = args.headless.expect("headless mode should be enabled");
    assert_eq!(
        headless.tas_project_path.as_deref(),
        Some(std::path::Path::new("movie.ztas"))
    );
    assert_eq!(headless.tas_branch_id.as_deref(), Some("route-b"));
    assert_eq!(
        headless.tas_export_path.as_deref(),
        Some(std::path::Path::new("verified.zrpl"))
    );
    assert_eq!(args.rom_path.as_deref(), Some("game.nes"));
}

#[test]
fn tas_project_verification_has_an_exact_cli_surface() {
    for args in [
        vec!["--tas-verify", "movie.ztas", "game.nes"],
        vec!["--headless", "--tas-verify", "movie.bin", "game.nes"],
        vec![
            "--headless",
            "--tas-verify",
            "movie.ztas",
            "--max-frames",
            "1",
            "game.nes",
        ],
        vec![
            "--headless",
            "--tas-verify",
            "movie.ztas",
            "game.nes",
            "other.nes",
        ],
        vec![
            "--headless",
            "--tas-verify",
            "movie.ztas",
            "--tas-export",
            "movie.bin",
            "game.nes",
        ],
    ] {
        assert!(parse_args_from(args).is_err());
    }
}

#[test]
fn tas_branch_and_export_require_tas_verify() {
    assert!(parse_args_from(["--headless", "--tas-branch", "main", "game.nes"]).is_err());
    assert!(parse_args_from(["--headless", "--tas-export", "movie.zrpl", "game.nes"]).is_err());
}

#[test]
fn parses_sgb_mode_override() {
    let args = parse_args_from(["--mode", "sgb", "game.gb"]).unwrap();

    assert_eq!(args.mode_override, Some(HardwareModePreference::ForceSgb));
}

#[test]
fn parses_headless_replay_options() {
    let args = parse_args_from([
        "--headless",
        "--replay",
        "test.zrpl",
        "--replay-peer",
        "peer.zrpl",
        "--replay-peer-live-link",
        "--replay-tail-frames",
        "12000",
        "--expect-gb-link-events",
        "2400",
        "--allow-gb-link-replay-divergence",
        "--expect-replay-final-hash",
        "abc123",
        "game.gb",
    ])
    .unwrap();

    let headless = args.headless.expect("headless mode should be enabled");
    assert_eq!(
        headless.replay_path.unwrap(),
        std::path::PathBuf::from("test.zrpl")
    );
    assert_eq!(
        headless.replay_peer_path.unwrap(),
        std::path::PathBuf::from("peer.zrpl")
    );
    assert!(headless.replay_peer_live_link);
    assert_eq!(headless.replay_tail_frames, 12_000);
    assert_eq!(headless.expect_gb_link_events, 2400);
    assert!(headless.allow_gb_link_replay_divergence);
    assert_eq!(headless.expect_replay_final_hash.as_deref(), Some("abc123"));
    assert_eq!(args.rom_path.as_deref(), Some("game.gb"));
}

#[test]
fn tas_script_rejects_external_input_in_either_order() {
    let path =
        std::env::temp_dir().join(format!("zeff-tas-parse-{}.ztascript", std::process::id()));
    std::fs::write(&path, "zeff-tas-script 1\nsystem gb\nframes 1\n").unwrap();
    let path = path.to_string_lossy().into_owned();

    let tas_first = parse_args_from([
        "--headless",
        "--tas-script",
        path.as_str(),
        "--press",
        "a@1",
        "game.gb",
    ]);
    let input_first = parse_args_from([
        "--headless",
        "--press",
        "a@1",
        "--tas-script",
        path.as_str(),
        "game.gb",
    ]);
    std::fs::remove_file(path).unwrap();

    assert!(tas_first.is_err());
    assert!(input_first.is_err());
}

#[test]
fn replay_peer_requires_primary_replay() {
    let err = match parse_args_from(["--headless", "--replay-peer", "peer.zrpl", "game.gb"]) {
        Ok(_) => panic!("--replay-peer without --replay should fail"),
        Err(err) => err,
    };
    assert!(format!("{err}").contains("--replay-peer requires --replay"));
}

#[test]
fn replay_peer_live_link_requires_replay_peer() {
    let err = match parse_args_from(["--headless", "--replay-peer-live-link", "game.gb"]) {
        Ok(_) => panic!("--replay-peer-live-link without --replay-peer should fail"),
        Err(err) => err,
    };
    assert!(format!("{err}").contains("--replay-peer-live-link requires --replay-peer"));
}

#[test]
fn zapper_events_accept_comma_coordinates_and_semicolon_separation() {
    let events = parse_zapper_event_arg("hit@240-242:128,96;miss@300:12x34", "--zapper").unwrap();

    assert_eq!(events.len(), 2);
    assert_eq!(events[0].start_frame, 240);
    assert_eq!(events[0].end_frame, 242);
    assert_eq!((events[0].x, events[0].y), (128, 96));
    assert!(events[0].trigger);
    assert!(events[0].hit);

    assert_eq!(events[1].start_frame, 300);
    assert_eq!(events[1].end_frame, 300);
    assert_eq!((events[1].x, events[1].y), (12, 34));
    assert!(events[1].trigger);
    assert!(!events[1].hit);
}

#[test]
fn sega8_video_standard_values_accept_region_aliases() {
    assert_eq!(
        parse_sega8_video_standard_arg("pal", "--sega8-video-standard").unwrap(),
        Sega8VideoStandard::Pal
    );
    assert_eq!(
        parse_sega8_video_standard_arg("60hz", "--sega8-video-standard").unwrap(),
        Sega8VideoStandard::Ntsc
    );
    assert!(parse_sega8_video_standard_arg("bad", "--sega8-video-standard").is_err());
}

#[test]
fn sega8_console_region_values_accept_aliases() {
    assert_eq!(
        parse_sega8_console_region_arg("japan", "--sega8-console-region").unwrap(),
        Sega8Region::Japanese
    );
    assert_eq!(
        parse_sega8_console_region_arg("international", "--sega8-console-region").unwrap(),
        Sega8Region::Export
    );
    assert_eq!(
        parse_sega8_console_region_arg("power-base", "--sega8-console-region").unwrap(),
        Sega8Region::JapanesePowerBaseConverter
    );
    assert!(parse_sega8_console_region_arg("bad", "--sega8-console-region").is_err());
}

#[test]
fn pce_controller_values_accept_host_facing_aliases() {
    assert_eq!(
        parse_pce_controller_mode_arg("pad", "--pce-controller").unwrap(),
        PceControllerMode::TwoButton
    );
    assert_eq!(
        parse_pce_controller_mode_arg("mouse", "--pce-controller").unwrap(),
        PceControllerMode::Mouse
    );
    assert_eq!(
        parse_pce_controller_mode_arg("6-button", "--pce-controller").unwrap(),
        PceControllerMode::SixButton
    );
    assert_eq!(
        parse_pce_controller_mode_arg("tap", "--pce-controller").unwrap(),
        PceControllerMode::Multitap
    );
    assert!(parse_pce_controller_mode_arg("bad", "--pce-controller").is_err());
}

#[test]
fn pce_memory_base_values_accept_host_facing_aliases() {
    assert_eq!(
        parse_pce_memory_base_mode_arg("auto", "--pce-memory-base").unwrap(),
        PceMemoryBaseMode::Automatic
    );
    assert_eq!(
        parse_pce_memory_base_mode_arg("on", "--pce-memory-base").unwrap(),
        PceMemoryBaseMode::Enabled
    );
    assert_eq!(
        parse_pce_memory_base_mode_arg("disabled", "--pce-memory-base").unwrap(),
        PceMemoryBaseMode::Disabled
    );
    assert!(parse_pce_memory_base_mode_arg("bad", "--pce-memory-base").is_err());
}

#[test]
fn parses_pce_memory_base_headless_option() {
    let args = parse_args_from(["--headless", "--pce-memory-base", "enabled", "game.pce"]).unwrap();
    assert_eq!(
        args.headless.unwrap().pce_memory_base_mode,
        Some(PceMemoryBaseMode::Enabled)
    );
}

#[test]
fn parses_pce_save_state_out_headless_option() {
    let args = parse_args_from([
        "--headless",
        "--pce-save-state-out",
        "target/checkpoint.pcestate",
        "game.pce",
    ])
    .unwrap();
    assert_eq!(
        args.headless.unwrap().pce_save_state_path,
        Some(std::path::PathBuf::from("target/checkpoint.pcestate"))
    );

    let err = match parse_args_from(["--headless", "--pce-save-state-out"]) {
        Ok(_) => panic!("the output option must require a path"),
        Err(err) => err,
    };
    assert!(
        err.to_string()
            .contains("--pce-save-state-out requires a file path")
    );
}

#[test]
fn parses_coleco_state_and_full_keypad_headless_options() {
    let args = parse_args_from([
        "--headless",
        "--coleco-save-state-out",
        "target/checkpoint.colstate",
        "--press",
        "star@10,pound@11,0@12,9@13",
        "game.col",
    ])
    .unwrap();
    let headless = args.headless.unwrap();

    assert_eq!(
        headless.coleco_save_state_path,
        Some(std::path::PathBuf::from("target/checkpoint.colstate"))
    );
    assert_eq!(
        headless
            .input_events
            .iter()
            .map(|event| event.coleco_keypad)
            .collect::<Vec<_>>(),
        [Some(10), Some(11), Some(0), Some(9)]
    );
}

#[test]
fn parses_coleco_audio_assertion() {
    let args = parse_args_from(["--headless", "--expect-coleco-audio", "game.col"]).unwrap();

    assert!(args.headless.unwrap().expect_coleco_audio);
}

#[test]
fn parses_pce_multitap_headless_inputs() {
    let args = parse_args_from([
        "--headless",
        "--press-p3",
        "a@1-2",
        "--press-p4",
        "b@3",
        "--press-p5",
        "run@4",
        "game.pce",
    ])
    .unwrap();
    let headless = args.headless.unwrap();
    assert_eq!(headless.input_events_p3.len(), 1);
    assert_eq!(headless.input_events_p4.len(), 1);
    assert_eq!(headless.input_events_p5.len(), 1);
}

#[test]
fn parses_headless_apply_mods_option() {
    let args = parse_args_from(["--headless", "--apply-mods", "game.pce"]).unwrap();
    assert!(args.headless.unwrap().apply_mods);
}

#[test]
fn region_dump_requires_a_bounded_nonempty_range() {
    let dump = parse_region_dump_arg("vram:0x7000:0x1000", "--dump-region").unwrap();
    assert_eq!(dump.region, "vram");
    assert_eq!(dump.offset, 0x7000);
    assert_eq!(dump.len, 0x1000);

    assert!(parse_region_dump_arg(":0:1", "--dump-region").is_err());
    assert!(parse_region_dump_arg("vram:0:0", "--dump-region").is_err());
    assert!(parse_region_dump_arg("vram:0:4097", "--dump-region").is_err());
    assert!(parse_region_dump_arg("vram:0", "--dump-region").is_err());
}

#[test]
fn parses_headless_region_dump_option() {
    let args = parse_args_from([
        "--headless",
        "--dump-region",
        "video_ram:0x7000:32",
        "game.pce",
    ])
    .unwrap();
    let dump = &args.headless.unwrap().region_dumps[0];
    assert_eq!(dump.region, "video_ram");
    assert_eq!(dump.offset, 0x7000);
    assert_eq!(dump.len, 32);
}

#[test]
fn pce_arcade_card_values_and_headless_option_accept_host_facing_aliases() {
    assert_eq!(
        parse_pce_arcade_card_mode_arg("auto", "--pce-arcade-card").unwrap(),
        PceArcadeCardMode::Automatic
    );
    assert_eq!(
        parse_pce_arcade_card_mode_arg("on", "--pce-arcade-card").unwrap(),
        PceArcadeCardMode::Enabled
    );
    assert_eq!(
        parse_pce_arcade_card_mode_arg("disabled", "--pce-arcade-card").unwrap(),
        PceArcadeCardMode::Disabled
    );
    assert!(parse_pce_arcade_card_mode_arg("bad", "--pce-arcade-card").is_err());

    let args = parse_args_from(["--headless", "--pce-arcade-card", "enabled", "game.cue"]).unwrap();
    assert_eq!(
        args.headless.unwrap().pce_arcade_card_mode,
        Some(PceArcadeCardMode::Enabled)
    );
}
