use super::HeadlessOptions;

pub(super) fn validate(
    headless_enabled: bool,
    rom_path: Option<&str>,
    options: &HeadlessOptions,
) -> anyhow::Result<()> {
    if options.audio_trace_path.is_none() {
        return Ok(());
    }
    anyhow::ensure!(headless_enabled, "--audio-trace requires --headless");
    anyhow::ensure!(
        rom_path.is_some(),
        "--audio-trace requires a cartridge path"
    );
    super::super::headless_runner::validate_audio_trace_options(options)
}

#[cfg(test)]
mod tests {
    use super::super::parse_args_from;

    #[test]
    fn audio_trace_requires_an_explicit_bounded_reset_run() {
        let parsed = parse_args_from([
            "--headless",
            "--audio-trace",
            "capture.zip",
            "--max-frames",
            "120",
            "game.sms",
        ])
        .unwrap();
        let options = parsed.headless.unwrap();
        assert_eq!(
            options.audio_trace_path.as_deref(),
            Some(std::path::Path::new("capture.zip"))
        );
        assert_eq!(options.max_frames, 120);
        for args in [
            vec!["--audio-trace", "capture.zip", "game.sms"],
            vec!["--headless", "--audio-trace"],
            vec!["--headless", "--audio-trace", "--no-sram", "game.sms"],
            vec!["--headless", "--audio-trace", "capture.zip"],
            vec!["--headless", "--audio-trace", "capture.vgm", "game.sms"],
            vec![
                "--headless",
                "--audio-trace",
                "a.zip",
                "--audio-trace",
                "b.zip",
                "game.sms",
            ],
            vec![
                "--headless",
                "--audio-trace",
                "capture.zip",
                "--max-frames",
                "0",
                "game.sms",
            ],
            vec![
                "--headless",
                "--audio-trace",
                "capture.zip",
                "--max-frames",
                "216001",
                "game.sms",
            ],
            vec![
                "--headless",
                "--audio-trace",
                "capture.zip",
                "--load-state",
                "state.bin",
                "game.sms",
            ],
            vec![
                "--headless",
                "--audio-trace",
                "capture.zip",
                "--replay",
                "movie.zrpl",
                "game.sms",
            ],
            vec![
                "--headless",
                "--audio-trace",
                "capture.zip",
                "--apply-mods",
                "game.sms",
            ],
        ] {
            assert!(parse_args_from(args.clone()).is_err(), "{args:?}");
        }
    }
}
