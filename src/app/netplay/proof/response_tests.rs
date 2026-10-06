use super::super::tests::{app, capture, connect_pair, wait};
use super::*;
use crate::netplay::Response;

#[test]
fn oversized_confirmed_pcm_stops_before_observer_hashing_or_copying() {
    rejected_response(Response::Frame {
        checkpoint: Message::Checkpoint {
            frame: 1,
            logical: [0; 32],
            video: [0; 32],
            audio: [0; 32],
            persistent: [0; 32],
        },
        ports: [0; 2],
        audio: vec![1.0; 4097],
    });
}

#[test]
fn audio_only_confirmation_stops_before_observer_hashing_or_copying() {
    rejected_response(Response::Audio {
        frame: 1,
        audio: vec![1.0; 100],
    });
}

fn rejected_response(response: Response) {
    let directory = crate::test_support::test_directory("cadence-response-rejection").unwrap();
    let mut one = app(directory.path(), "one");
    let mut two = app(directory.path(), "two");
    one.netplay.proof = Some(Observation {
        ledger: Some(cadence::ledger::Ledger::new(120).unwrap()),
        ..Observation::default()
    });
    let initial = connect_pair(&mut one, &mut two);
    let paths = [
        one.rom_info.rom_path.clone().unwrap(),
        two.rom_info.rom_path.clone().unwrap(),
    ];
    let saves = paths
        .each_ref()
        .map(|path| media::optional_save(path).unwrap());
    assert!(
        one.consume_netplay_response(EmuResponse::Netplay(response))
            .is_none()
    );
    assert!(one.netplay.phase == Phase::Stopping);
    let proof = one.netplay.proof.as_ref().unwrap();
    assert_eq!(proof.frames, 0);
    assert!(proof.last.is_none());
    assert_eq!(proof.pcm_hash.clone().finalize(), Sha256::new().finalize());
    let ledger = proof.ledger.as_ref().unwrap();
    assert!(ledger.check_error().is_err());
    assert!(ledger.records().is_empty());
    assert!(one.netplay.observed_frames.is_empty());
    assert!(one.netplay.audio_prebuffer.is_empty());
    assert!(one.netplay.queued_audio.is_none());
    wait(&mut one, |app| app.netplay.phase == Phase::Idle);
    wait(&mut two, |app| app.netplay.phase == Phase::Idle);
    assert_eq!(capture(&mut one), initial.0);
    assert_eq!(capture(&mut two), initial.1);
    one.stop_emu_thread();
    two.stop_emu_thread();
    for (path, save) in paths.iter().zip(saves) {
        assert_eq!(media::optional_save(path).unwrap(), save);
    }
}
