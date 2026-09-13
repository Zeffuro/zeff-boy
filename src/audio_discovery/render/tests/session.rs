use super::*;

fn read_session(session: &mut RenderSession, chunk_samples: usize) -> Result<Vec<i16>> {
    let cancel = AtomicBool::new(false);
    let mut pcm = Vec::new();
    let mut buffer = vec![0; chunk_samples];
    loop {
        let written = session.read(&mut buffer, &cancel)?;
        if written == 0 {
            return Ok(pcm);
        }
        pcm.extend_from_slice(&buffer[..written]);
    }
}

#[test]
fn incremental_session_matches_streamed_render_for_varied_reads_and_reset_seek() -> Result<()> {
    let (song, bytes, bank) = fixture_inputs(0, 8_000, 127, 127)?;
    let options = RenderOptions {
        max_seconds: 2,
        ..RenderOptions::default()
    };
    let expected = render(
        &song,
        &bytes,
        &bank,
        options,
        &AtomicBool::new(false),
        &AtomicU32::new(0),
    )?;
    for chunk_samples in [16, 34, 510, 8192] {
        let mut session =
            RenderSession::new(&song, &bytes, &bank, options, &AtomicBool::new(false))?;
        assert_eq!(session.sample_rate(), options.sample_rate);
        assert_eq!(session.track_count(), song.tracks.len());
        assert_eq!(session.duration_frames(), expected.pcm.len() / 2);
        assert_eq!(read_session(&mut session, chunk_samples)?, expected.pcm);
        session.reset()?;
        let mut discard = [0; 16];
        for _ in 0..32 {
            session.read(&mut discard, &AtomicBool::new(false))?;
        }
        let position = session.position_frames();
        let suffix = read_session(&mut session, chunk_samples)?;
        assert_eq!(suffix, expected.pcm[position * 2..]);
    }
    Ok(())
}

#[test]
fn muted_tracks_stay_silent_without_changing_duration_or_cancellation_contract() -> Result<()> {
    let (song, bytes, bank) = fixture_inputs(0, 8_000, 127, 127)?;
    let options = RenderOptions {
        max_seconds: 2,
        ..RenderOptions::default()
    };
    let mut muted = RenderSession::new(&song, &bytes, &bank, options, &AtomicBool::new(false))?;
    muted.set_track_mask(0)?;
    let duration = muted.duration_frames();
    let pcm = read_session(&mut muted, 510)?;
    assert_eq!(pcm.len() / 2, duration);
    assert!(pcm.iter().all(|sample| *sample == 0));
    assert!(muted.set_track_mask(2).is_err());
    let cancelled = AtomicBool::new(true);
    assert!(muted.read(&mut [0; 16], &cancelled).is_err());
    assert!(RenderSession::new(&song, &bytes, &bank, options, &cancelled).is_err());
    Ok(())
}

fn pcm_energy(pcm: &[i16]) -> u64 {
    pcm.iter()
        .map(|sample| u64::from(sample.unsigned_abs()))
        .sum()
}

#[test]
fn muting_a_tempo_track_preserves_duration_and_solo_tracks_remain_audible() -> Result<()> {
    let (song, bytes, bank) = two_track_fixture_inputs()?;
    let options = RenderOptions {
        max_seconds: 2,
        ..RenderOptions::default()
    };
    let cancel = AtomicBool::new(false);
    let mut mixed = RenderSession::new(&song, &bytes, &bank, options, &cancel)?;
    let mut without_tempo_track = RenderSession::new(&song, &bytes, &bank, options, &cancel)?;
    without_tempo_track.set_track_mask(0b01)?;
    let mut tempo_track_solo = RenderSession::new(&song, &bytes, &bank, options, &cancel)?;
    tempo_track_solo.set_track_mask(0b10)?;

    assert_eq!(song.tracks.len(), 2);
    assert_eq!(
        without_tempo_track.duration_frames(),
        mixed.duration_frames()
    );
    assert_eq!(tempo_track_solo.duration_frames(), mixed.duration_frames());

    let mixed_pcm = read_session(&mut mixed, 510)?;
    let without_tempo_pcm = read_session(&mut without_tempo_track, 510)?;
    let tempo_solo_pcm = read_session(&mut tempo_track_solo, 510)?;
    assert!(pcm_energy(&without_tempo_pcm) > 0);
    assert!(pcm_energy(&tempo_solo_pcm) > 0);
    assert_ne!(mixed_pcm, without_tempo_pcm);
    assert_ne!(without_tempo_pcm, tempo_solo_pcm);
    Ok(())
}

#[test]
fn unmuting_restores_a_note_and_cc7_that_were_dispatched_while_a_track_was_active() -> Result<()> {
    let (song, bytes, bank) = two_track_fixture_inputs()?;
    let options = RenderOptions {
        max_seconds: 2,
        ..RenderOptions::default()
    };
    let cancel = AtomicBool::new(false);
    let mut session = RenderSession::new(&song, &bytes, &bank, options, &cancel)?;
    session.set_track_mask(0b01)?;

    let audible_deadline = session
        .position_frames()
        .saturating_add(session.sample_rate() as usize / 4)
        .min(session.duration_frames());
    let mut initial = [0; 512];
    let mut sounded = false;
    while session.position_frames() < audible_deadline {
        let initial_samples = session.read(&mut initial, &cancel)?;
        sounded |= pcm_energy(&initial[..initial_samples]) > 0;
        if sounded {
            break;
        }
    }
    assert!(sounded, "the note and CC7 must be audible before muting");

    session.set_track_mask(0)?;
    let mut muted = [0; 512];
    let muted_samples = session.read(&mut muted, &cancel)?;
    assert!(
        muted[(SYNTH_BLOCK_FRAMES * 2).min(muted_samples)..muted_samples]
            .iter()
            .all(|sample| *sample == 0),
        "the CC11 mute must settle within one synthesis block"
    );

    session.set_track_mask(0b01)?;
    let mut restored = [0; 512];
    let restored_samples = session.read(&mut restored, &cancel)?;
    assert!(
        pcm_energy(&restored[..restored_samples]) > 0,
        "unmuting must restore the already sounding note at its CC7 volume"
    );
    Ok(())
}
