use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

use super::*;
use crate::audio_discovery::{ScanLimits, media::ScanInput, render::RenderOptions};

mod cdda;
mod pcm;

#[test]
fn driver_control_entries_remain_inspectable_but_cannot_start_silent_playback() {
    use crate::audio_discovery::{
        RomSpan,
        catalog::SongId,
        natsume::{NatsumeSong, NatsumeSongKind, PROFILE},
    };
    let (source, mut manifest) = fixture();
    assert!(PreviewRequest::can_preview(&manifest, SongId::Mp2k(0)));
    assert!(!PreviewRequest::can_preview(
        &manifest,
        SongId::Mp2k(usize::MAX)
    ));
    for (index, kind) in [
        NatsumeSongKind::Setup,
        NatsumeSongKind::Setup,
        NatsumeSongKind::Music,
        NatsumeSongKind::Silence,
    ]
    .into_iter()
    .enumerate()
    {
        manifest.scan.natsume_songs.push(NatsumeSong {
            profile: PROFILE,
            index: index as u16,
            title: String::new(),
            kind,
            table_entry: RomSpan {
                effective_offset: 0x100,
                canonical_cpu_address: 0x0800_0100,
                byte_len: 4,
            },
            header: RomSpan {
                effective_offset: 0x200,
                canonical_cpu_address: 0x0800_0200,
                byte_len: 4,
            },
            channel_mask: 1,
            priority: 0,
            channels: Vec::new(),
            mapped_spans: Vec::new(),
            warnings: Vec::new(),
        });
        let selection = SongId::Natsume(index);
        assert!(manifest.scan.song(selection).is_some());
        assert_eq!(
            PreviewRequest::can_preview(&manifest, selection),
            index == 2
        );
        let request =
            PreviewRequest::prepare_song(&source, &manifest, selection, RenderOptions::default());
        if index == 2 {
            assert!(request.is_ok());
        } else {
            assert!(
                request
                    .err()
                    .unwrap()
                    .to_string()
                    .contains("driver control")
            );
        }
    }
}

pub(crate) fn fixture() -> (Arc<ScanInput>, crate::audio_discovery::media::ScanManifest) {
    let mut bytes = crate::audio_discovery::test_support::gba_fixture();
    // Explicit volume and a sustained note make audible progress independently observable.
    for offset in [0x400, 0x440] {
        bytes[offset..offset + 14].copy_from_slice(&[
            0xBB, 75, 0xBD, 0, 0xBE, 127, 0xFF, 60, 127, 0xB0, 0xB0, 0xB0, 0xB0, 0xB1,
        ]);
    }
    let input = Arc::new(ScanInput {
        cdda: None,
        system: Some(zeff_emu_common::system::System::Gba),
        standalone_audio: None,
        bytes: bytes.into(),
        provenance: None,
        analysis_profile: "preview-test",
        display_name: None,
    });
    let manifest = input.analyze(ScanLimits::default(), &AtomicBool::new(false));
    assert!(!manifest.scan.candidates.is_empty());
    (input, manifest)
}

fn request() -> PreviewRequest {
    let (source, manifest) = fixture();
    PreviewRequest::prepare(
        &source,
        &manifest,
        0,
        RenderOptions {
            max_seconds: 2,
            ..Default::default()
        },
    )
    .unwrap()
}

fn wait(player: &mut PreviewPlayer, condition: impl Fn(&PreviewPlayer) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !condition(player) {
        player.poll();
        assert!(player.error.is_none(), "{:?}", player.error);
        assert!(Instant::now() < deadline, "preview wait timed out");
        std::thread::yield_now();
    }
}

#[test]
fn bounded_preview_transport_pause_seek_restart_and_stop() {
    let mut player = PreviewPlayer::default();
    player.set_volume(100);
    let receiver = player.start_captured(request());
    let mut callback = receiver.recv_timeout(Duration::from_secs(10)).unwrap();
    wait(&mut player, |player| {
        player.snapshot().is_some_and(|s| !s.preparing)
    });
    let mut output = [0.0f32; 128];
    let deadline = Instant::now() + Duration::from_secs(10);
    while player.snapshot().unwrap().position < 2048 {
        callback.fill(&mut output, 2);
        assert!(Instant::now() < deadline);
        std::thread::yield_now();
    }
    player.set_playing(false);
    let paused = player.snapshot().unwrap().position;
    callback.fill(&mut output, 2);
    assert!(output.iter().all(|sample| *sample == 0.0));
    assert_eq!(player.snapshot().unwrap().position, paused);
    for target in [16_000, 8_000, 24_000, 0] {
        player.seek(target);
    }
    wait(&mut player, |player| {
        player
            .snapshot()
            .is_some_and(|s| !s.preparing && s.position == 0)
    });
    callback.fill(&mut output, 2);
    assert!(output.iter().all(|sample| *sample == 0.0));
    player.set_playing(true);
    player.set_track_mask(0);
    player.stop();
    callback.fill(&mut output, 2);
    assert!(output.iter().all(|sample| *sample == 0.0));
    assert!(player.snapshot().is_none());
    wait(&mut player, |player| !player.is_pending());
}

#[test]
fn rapid_replacement_retains_one_worker_and_only_latest_request() {
    let mut player = PreviewPlayer::default();
    let first = player.start_captured(request());
    let mut first_callback = first.recv_timeout(Duration::from_secs(10)).unwrap();
    let second = player.start_captured(request());
    let latest = player.start_captured(request());
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut latest_callback = loop {
        player.poll();
        if let Ok(callback) = latest.try_recv() {
            break callback;
        }
        assert!(Instant::now() < deadline);
        std::thread::yield_now();
    };
    drop(second);
    let mut output = [1.0f32; 32];
    first_callback.fill(&mut output, 2);
    assert_eq!(output, [0.0; 32]);
    drop(player);
    latest_callback.fill(&mut output, 2);
    assert_eq!(output, [0.0; 32]);
}

#[test]
fn source_identity_and_device_failure_are_explicit_and_recoverable() {
    let (source, mut manifest) = fixture();
    manifest.scan.media.sha256 = Some("0".repeat(64));
    let request = PreviewRequest::prepare(&source, &manifest, 0, RenderOptions::default()).unwrap();
    assert!(request.renderer(48_000, &AtomicBool::new(false)).is_err());
    let mut player = PreviewPlayer::default();
    let receiver = player.start_captured(super::tests::request());
    let _callback = receiver.recv_timeout(Duration::from_secs(10)).unwrap();
    player
        .job
        .as_ref()
        .unwrap()
        .shared
        .device_error
        .store(true, Ordering::Release);
    let deadline = Instant::now() + Duration::from_secs(10);
    while player.is_pending() {
        player.poll();
        assert!(Instant::now() < deadline);
        std::thread::yield_now();
    }
    assert!(
        player
            .error
            .as_ref()
            .unwrap()
            .contains("audio output device stopped")
    );
    let _receiver = player.start_captured(super::tests::request());
    assert!(player.error.is_none());
}

fn consumed_pcm(
    player: &mut PreviewPlayer,
    callback: &mut output::Callback,
    frames: usize,
) -> Vec<f32> {
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut pcm = Vec::with_capacity(frames * 2);
    while pcm.len() < frames * 2 {
        player.poll();
        assert!(player.error.is_none(), "{:?}", player.error);
        let before = player.snapshot().unwrap().position;
        let mut frame = [0.0f32; 2];
        callback.fill(&mut frame, 2);
        if player.snapshot().unwrap().position != before {
            pcm.extend(frame);
        } else {
            std::thread::yield_now();
        }
        assert!(
            Instant::now() < deadline,
            "callback consumed {} of {} samples: {:?}",
            pcm.len(),
            frames * 2,
            player.snapshot()
        );
    }
    pcm
}

#[test]
fn preview_pcm_matches_renderer_after_pause_seek_and_restart_and_mute_is_bounded() {
    let cancel = AtomicBool::new(false);
    let mut renderer = request().renderer(48_000, &cancel).unwrap();
    let mut expected = Vec::new();
    let mut buffer = [0; 512];
    while renderer.position_frames() < 16_000 {
        let samples = renderer.read(&mut buffer, &cancel).unwrap();
        expected.extend(
            buffer[..samples]
                .iter()
                .map(|sample| f32::from(*sample) / 32768.0),
        );
    }
    let mut player = PreviewPlayer::default();
    player.set_volume(100);
    let receiver = player.start_captured(request());
    let mut callback = receiver.recv_timeout(Duration::from_secs(10)).unwrap();
    assert_eq!(
        consumed_pcm(&mut player, &mut callback, 2048),
        expected[..4096]
    );
    player.set_playing(false);
    player.seek(8_000);
    wait(&mut player, |player| {
        player.snapshot().is_some_and(|state| !state.preparing)
    });
    let mut silence = [1.0f32; 2];
    callback.fill(&mut silence, 2);
    assert_eq!(silence, [0.0; 2]);
    assert_eq!(player.snapshot().unwrap().position, 8_000);
    player.set_playing(true);
    assert_eq!(
        consumed_pcm(&mut player, &mut callback, 1024),
        expected[16_000..18_048]
    );
    player.seek(0);
    assert_eq!(
        consumed_pcm(&mut player, &mut callback, 1024),
        expected[..2048]
    );
    player.set_track_mask(0);
    let muted = consumed_pcm(&mut player, &mut callback, 4096);
    let settling_frames = player.job.as_ref().unwrap().shared.preroll_frames() + 256 + 1 + 8;
    assert!(
        muted[settling_frames * 2..]
            .iter()
            .all(|sample| *sample == 0.0)
    );
    player.set_track_mask(u16::MAX);
    let unmuted = consumed_pcm(&mut player, &mut callback, 4096);
    assert!(
        unmuted[settling_frames * 2..]
            .iter()
            .any(|sample| *sample != 0.0)
    );
}
