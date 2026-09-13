use super::*;
use crate::audio_discovery::{formats::AudioFormat, pcm::write_new};
use std::sync::atomic::AtomicU32;

fn fixture() -> PreparedGbNative {
    let bytes = zeff_audio_discovery::gb_native::fixture_rom();
    let report = zeff_audio_discovery::scan(
        zeff_emu_common::system::System::Gb,
        &bytes,
        Default::default(),
        &AtomicBool::new(false),
    );
    zeff_audio_discovery::gb_native::prepare_rom(
        &bytes,
        &report.gb_native_songs[0],
        &AtomicBool::new(false),
    )
    .unwrap()
}
fn options() -> RenderOptions {
    RenderOptions {
        max_seconds: 1,
        sample_rate: 44_100,
        ..Default::default()
    }
}
fn make() -> Result<GbSession> {
    GbSession::new(fixture(), options(), Vec::new(), &AtomicBool::new(false))
}
fn render(session: &mut GbSession, block: usize) -> Result<Vec<i16>> {
    let mut result = Vec::new();
    let mut chunk = vec![0; block];
    loop {
        let count = session.read(&mut chunk, &AtomicBool::new(false))?;
        if count == 0 {
            return Ok(result);
        }
        result.extend_from_slice(&chunk[..count]);
    }
}
#[test]
fn native_handoff_reset_and_recording_match() -> Result<()> {
    let mut session = make()?;
    assert_eq!(session.read(&mut [], &AtomicBool::new(false))?, 0);
    assert!(session.read(&mut [0; 64], &AtomicBool::new(true)).is_err());
    assert_eq!(session.position_frames(), 0);
    let expected = render(&mut session, 2048)?;
    assert_eq!(expected.len(), 88_200);
    assert!(expected.iter().any(|sample| *sample != 0));
    assert_eq!(session.emulator.hardware_mode(), HardwareMode::DMG);
    assert_eq!(session.emulator.cpu_peek8(0xc000), 0xba);
    session.reset()?;
    assert_eq!(render(&mut session, 258)?, expected);
    session.set_track_mask(0)?;
    session.reset()?;
    assert!(render(&mut session, 512)?.iter().all(|sample| *sample == 0));
    assert!(session.set_track_mask(2).is_err());
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("gb.wav");
    write_new(
        Box::new(make()?),
        AudioFormat::Wav,
        options(),
        serde_json::json!({}),
        &path,
        &AtomicBool::new(false),
        &AtomicU32::new(0),
    )?;
    assert_eq!(
        hound::WavReader::open(path)?
            .samples::<i16>()
            .collect::<std::result::Result<Vec<_>, _>>()?,
        expected
    );
    Ok(())
}
#[test]
fn ready_marker_alone_does_not_release_the_song() -> Result<()> {
    let mut prepared = fixture();
    prepared.wait_start = 0x3e00;
    prepared.wait_end = 0x3e04;
    let ack = prepared.ack_address;
    let mut session = GbSession::new(prepared, options(), Vec::new(), &AtomicBool::new(false))?;
    assert!(session.read(&mut [0; 64], &AtomicBool::new(false)).is_err());
    assert_eq!(session.emulator.cpu_peek8(ack), 0);
    assert_eq!(session.position_frames(), 0);
    Ok(())
}

#[test]
fn finite_cue_caps_preview_and_recording_to_the_qualified_end() -> Result<()> {
    let bytes = zeff_audio_discovery::gb_native::fixture_rom();
    let cancel = AtomicBool::new(false);
    let report = zeff_audio_discovery::scan(
        zeff_emu_common::system::System::Gb,
        &bytes,
        Default::default(),
        &cancel,
    );
    let song = report
        .gb_native_songs
        .iter()
        .find(|song| song.raw_index == 0xe8)
        .unwrap();
    assert_eq!(song.playback_frames, 136);
    assert_eq!(song.loop_start_frame, None);
    let options = RenderOptions {
        max_seconds: 10,
        fade_seconds: 1,
        ..options()
    };
    let make = || {
        GbSession::new(
            zeff_audio_discovery::gb_native::prepare_rom(&bytes, song, &cancel)?,
            options,
            Vec::new(),
            &cancel,
        )
    };
    let mut session = make()?;
    let frames = song.playback_clocks * u64::from(options.sample_rate) / 4_194_304;
    assert_eq!(session.duration_frames(), frames as usize);
    let expected = render(&mut session, 258)?;
    assert_eq!(expected.len(), frames as usize * 2);
    assert!(expected.iter().any(|&sample| sample != 0));
    assert_eq!(&expected[expected.len() - 2..], &[0, 0]);
    assert_eq!(session.emulator.cpu_peek8(0xc001), 136);
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("finite.wav");
    write_new(
        Box::new(make()?),
        AudioFormat::Wav,
        options,
        serde_json::json!({}),
        &path,
        &cancel,
        &AtomicU32::new(0),
    )?;
    let mut wave = hound::WavReader::open(path)?;
    assert_eq!(u64::from(wave.duration()), frames);
    assert_eq!(
        wave.samples::<i16>()
            .collect::<std::result::Result<Vec<_>, _>>()?,
        expected
    );
    Ok(())
}

#[test]
fn native_cartridge_profile_and_zero_duration_are_rejected() {
    let mut prepared = fixture();
    prepared.bytes[0x147] = 0;
    assert!(GbSession::new(prepared, options(), Vec::new(), &AtomicBool::new(false)).is_err());
    let mut prepared = fixture();
    prepared.playback_frames = 0;
    assert!(GbSession::new(prepared, options(), Vec::new(), &AtomicBool::new(false)).is_err());
}

fn cgb_fixture() -> PreparedGbNative {
    cgb_fixture_from(&zeff_audio_discovery::gb_native::cgb_fixture_rom())
}

fn cgb_fixture_from(bytes: &[u8]) -> PreparedGbNative {
    let cancel = AtomicBool::new(false);
    let report = zeff_audio_discovery::scan(
        zeff_emu_common::system::System::Gb,
        bytes,
        Default::default(),
        &cancel,
    );
    zeff_audio_discovery::gb_native::prepare_rom(bytes, &report.gb_native_songs[0], &cancel)
        .unwrap()
}

#[test]
fn cgb_original_speed_switch_reset_and_recording_match() -> Result<()> {
    let cancel = AtomicBool::new(false);
    for bytes in [
        zeff_audio_discovery::gb_native::cgb_fixture_rom(),
        zeff_audio_discovery::gb_native::cgb_ram_fixture_rom(),
    ] {
        let make = || {
            let mut prepared = cgb_fixture_from(&bytes);
            prepared.playback_clocks = 2_097_152;
            GbSession::new(prepared, options(), Vec::new(), &cancel)
        };
        let mut session = make()?;
        assert_eq!(session.emulator.hardware_mode(), HardwareMode::CGBNormal);
        assert_eq!(session.duration_frames(), 11_025);
        assert!(
            session
                .warnings()
                .iter()
                .any(|s| s.contains("CGB double-speed"))
        );
        assert!(session.read(&mut [0; 64], &AtomicBool::new(true)).is_err());
        assert_eq!(session.position_frames(), 0);
        let expected = render(&mut session, 258)?;
        assert_eq!(expected.len(), 22_050);
        assert!(expected.iter().any(|&sample| sample != 0));
        assert_eq!(session.emulator.hardware_mode(), HardwareMode::CGBDouble);
        session.reset()?;
        assert_eq!(session.emulator.hardware_mode(), HardwareMode::CGBNormal);
        assert_eq!(render(&mut session, 4096)?, expected);
        session.set_track_mask(0)?;
        session.reset()?;
        assert!(render(&mut session, 512)?.iter().all(|&sample| sample == 0));
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("cgb.wav");
        write_new(
            Box::new(make()?),
            AudioFormat::Wav,
            options(),
            serde_json::json!({}),
            &path,
            &cancel,
            &AtomicU32::new(0),
        )?;
        let mut wave = hound::WavReader::open(path)?;
        assert_eq!(wave.duration(), 11_025);
        assert_eq!(
            wave.samples::<i16>()
                .collect::<std::result::Result<Vec<_>, _>>()?,
            expected
        );
    }
    Ok(())
}

#[test]
fn cgb_ready_handoff_requires_the_original_speed_switch() -> Result<()> {
    let mut prepared = cgb_fixture();
    let ready = prepared.ready_address.to_le_bytes();
    let ack = prepared.ack_address.to_le_bytes();
    let code = [
        0xf3,
        0x3e,
        prepared.ready_value,
        0xea,
        ready[0],
        ready[1],
        0xfa,
        ack[0],
        ack[1],
        0xfe,
        prepared.ack_value,
        0x20,
        0xf9,
    ];
    prepared.bytes[0x100..0x103].copy_from_slice(&[0xc3, 0, 3]);
    prepared.bytes[0x300..0x300 + code.len()].copy_from_slice(&code);
    prepared.wait_start = 0x306;
    prepared.wait_end = 0x30d;
    let ack = prepared.ack_address;
    let mut session = GbSession::new(prepared, options(), Vec::new(), &AtomicBool::new(false))?;
    let error = session
        .read(&mut [0; 64], &AtomicBool::new(false))
        .unwrap_err();
    assert!(error.to_string().contains("qualified playback speed"));
    assert_eq!(session.emulator.hardware_mode(), HardwareMode::CGBNormal);
    assert_eq!(session.emulator.cpu_peek8(ack), 0);
    assert_eq!(session.position_frames(), 0);
    Ok(())
}

#[test]
fn cgb_timing_and_cartridge_mismatches_are_rejected() {
    for (offset, value) in [(0x143, 0x80), (0x147, 0x13), (0x148, 5), (0x149, 3)] {
        let mut prepared = cgb_fixture();
        prepared.bytes[offset] = value;
        assert!(GbSession::new(prepared, options(), Vec::new(), &AtomicBool::new(false)).is_err());
    }
    let mut prepared = cgb_fixture();
    prepared.timing = GbNativeTiming::Dmg;
    assert!(GbSession::new(prepared, options(), Vec::new(), &AtomicBool::new(false)).is_err());
}

#[test]
fn cgb_qualified_ends_and_loop_bound_use_cpu_clock_durations() -> Result<()> {
    let bytes = zeff_audio_discovery::gb_native::cgb_fixture_rom();
    let cancel = AtomicBool::new(false);
    let report = zeff_audio_discovery::scan(
        zeff_emu_common::system::System::Gb,
        &bytes,
        Default::default(),
        &cancel,
    );
    let options = RenderOptions {
        max_seconds: 10,
        fade_seconds: 1,
        ..options()
    };
    assert_eq!(report.gb_native_songs.len(), 3);
    for (song, frames) in report
        .gb_native_songs
        .iter()
        .zip([100_417, 50_209, 189_758])
    {
        let mut session = GbSession::new(
            zeff_audio_discovery::gb_native::prepare_rom(&bytes, song, &cancel)?,
            options,
            Vec::new(),
            &cancel,
        )?;
        assert_eq!(session.duration_frames(), frames);
        let pcm = render(&mut session, 2048)?;
        assert_eq!(pcm.len(), frames * 2);
        assert!(pcm.iter().any(|&sample| sample != 0));
        assert_eq!(&pcm[pcm.len() - 2..], &[0, 0]);
        assert_eq!(
            session.emulator.cpu_peek8(0xcc14) & 1 != 0,
            song.loop_start_frame.is_some()
        );
    }
    Ok(())
}
