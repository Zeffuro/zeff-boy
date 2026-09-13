use std::sync::atomic::AtomicU32;

use super::*;
use crate::audio_discovery::{formats::AudioFormat, pcm::write_new};

fn fixture(system: System) -> PreparedSegaPsg {
    let mut bytes = vec![0; 0x8000];
    bytes[..3].copy_from_slice(&[0xc3, 0x00, 0x01]);
    let code = [
        0xf3, 0x31, 0xf0, 0xdf, 0xaf, 0x32, 0x01, 0xc0, 0x3e, 0x01, 0x32, 0x00, 0xc0, 0x3a, 0x01,
        0xc0, 0xfe, 0x01, 0x20, 0xf9, 0x3e, 0x10, 0xd3, 0x06, 0x3e, 0x80, 0xd3, 0x7f, 0x3e, 0x10,
        0xd3, 0x7f, 0x3e, 0x90, 0xd3, 0x7f, 0x76, 0x18, 0xfd,
    ];
    bytes[0x100..0x100 + code.len()].copy_from_slice(&code);
    PreparedSegaPsg {
        bytes,
        system,
        region: SegaPsgRegion::Export,
        timing: SegaPsgTiming::Ntsc,
        ready_address: 0xc000,
        ack_address: 0xc001,
        wait_start: 0x10d,
        wait_end: 0x114,
    }
}

fn options() -> RenderOptions {
    RenderOptions {
        max_seconds: 1,
        sample_rate: 44_100,
        ..Default::default()
    }
}

fn make(system: System) -> Result<SegaSession> {
    SegaSession::new(
        fixture(system),
        options(),
        Vec::new(),
        &AtomicBool::new(false),
    )
}

fn render(session: &mut SegaSession, block: usize) -> Result<Vec<i16>> {
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
fn native_stereo_handoff_reset_and_recording_match() -> Result<()> {
    for system in [System::Sms, System::Gg] {
        let mut session = make(system)?;
        assert_eq!(session.read(&mut [], &AtomicBool::new(false))?, 0);
        assert!(session.read(&mut [0; 64], &AtomicBool::new(true)).is_err());
        assert_eq!(session.position_frames(), 0);
        let expected = render(&mut session, 2048)?;
        assert_eq!(expected.len(), 88_200);
        assert!(
            expected
                .as_chunks::<2>()
                .0
                .iter()
                .any(|frame| frame[0] != 0)
        );
        assert!(expected.as_chunks::<2>().0.iter().all(|frame| {
            if system == System::Gg {
                frame[1] == 0
            } else {
                frame[0] == frame[1]
            }
        }));
        session.reset()?;
        assert_eq!(render(&mut session, 258)?, expected);
        session.set_track_mask(0)?;
        session.reset()?;
        assert!(render(&mut session, 512)?.iter().all(|sample| *sample == 0));
        assert!(session.set_track_mask(2).is_err());
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("sega.wav");
        write_new(
            Box::new(make(system)?),
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
    }
    Ok(())
}

#[test]
fn ready_marker_alone_does_not_release_the_song() -> Result<()> {
    let mut prepared = fixture(System::Sms);
    prepared.wait_start = 0x200;
    prepared.wait_end = 0x204;
    let mut session = SegaSession::new(prepared, options(), Vec::new(), &AtomicBool::new(false))?;
    assert!(session.read(&mut [0; 64], &AtomicBool::new(false)).is_err());
    assert_eq!(session.emulator.cpu_peek8(0xc001), 0);
    assert_eq!(session.position_frames(), 0);
    Ok(())
}
