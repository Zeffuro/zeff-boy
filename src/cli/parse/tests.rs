use super::input::parse_zapper_event_arg;
use super::parse_args_from;
use super::values::{parse_sega8_console_region_arg, parse_sega8_video_standard_arg};
use zeff_sega8_core::hardware::region::Sega8Region;
use zeff_sega8_core::hardware::timing::Sega8VideoStandard;

#[test]
fn parses_headless_replay_options() {
    let args = parse_args_from([
        "--headless",
        "--replay",
        "test.zrpl",
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
    assert_eq!(headless.expect_replay_final_hash.as_deref(), Some("abc123"));
    assert_eq!(args.rom_path.as_deref(), Some("game.gb"));
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
