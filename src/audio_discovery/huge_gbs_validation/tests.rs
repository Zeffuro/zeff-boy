use std::sync::atomic::AtomicBool;

use anyhow::Result;
use zeff_audio_discovery::scan;
use zeff_emu_common::system::System;

use super::*;

fn fixture() -> (Vec<u8>, HugeSong) {
    let bytes = zeff_audio_discovery::huge::fixture::rom();
    let mut report = scan(
        System::Gb,
        &bytes,
        Default::default(),
        &AtomicBool::new(false),
    );
    (bytes, report.huge_songs.remove(0))
}

#[test]
fn returning_gbs_matches_native_calls_and_direct_host_pcm() -> Result<()> {
    let (bytes, song) = fixture();
    let (artifact, report) = validate(&bytes, &song, &AtomicBool::new(false))?;
    assert_eq!(report["native_calls"], song.validation_frames + 1);
    assert_eq!(report["layouts"].as_array().unwrap().len(), 2);
    assert_eq!(&artifact.bytes[..6], b"GBS\x01\x01\x01");
    assert!(
        zeff_audio_discovery::catalog::SongRef::Huge(&song)
            .supports(zeff_audio_discovery::formats::SongFormat::Gbs)
    );
    Ok(())
}

#[test]
fn validate_rejects_stale_or_forged_selection_at_the_builder_gate() {
    let (bytes, song) = fixture();
    let cancel = AtomicBool::new(false);

    let mut stale_bytes = bytes.clone();
    stale_bytes[0] ^= 1;
    assert!(
        validate(&stale_bytes, &song, &cancel)
            .unwrap_err()
            .to_string()
            .contains("source hash no longer matches")
    );

    let mut forged_bound_song = song.clone();
    forged_bound_song.bound.evidence.ram_address ^= 1;
    assert!(
        validate(&bytes, &forged_bound_song, &cancel)
            .unwrap_err()
            .to_string()
            .contains("one exact descriptor binding")
    );

    let mut stale_budget = song.clone();
    stale_budget.validation_frames -= 1;
    assert!(
        validate(&bytes, &stale_budget, &cancel)
            .unwrap_err()
            .to_string()
            .contains("validation budget no longer matches")
    );
}

#[test]
fn host_rejects_changed_schedule_returns_and_source_reads() -> Result<()> {
    let (bytes, song) = fixture();
    let cancel = AtomicBool::new(false);
    let native = native::collect(&bytes, &song, &cancel)?;
    let artifact = || gbs::build(&bytes, &song, &cancel);
    let mut changed = artifact()?;
    changed.bytes[15] = 4;
    assert!(host::Host::artifact(&changed, &song, 0).is_err());
    for opcode in [0xd9, 0x76] {
        let mut changed = artifact()?;
        let last = (changed.play_wrapper.offset + changed.play_wrapper.byte_len - 1) as usize;
        changed.bytes[last - 0x400 + 0x70] = opcode;
        assert!(
            host::Host::artifact(&changed, &song, 0)?
                .run(&native, &cancel)
                .is_err()
        );
    }
    let mut changed = artifact()?;
    let at = changed.driver_start as usize - 0x400 + 0x70;
    changed.bytes[at..at + 2].copy_from_slice(&[0xe1, 0xe9]);
    let error = host::Host::artifact(&changed, &song, 0)?
        .run(&native, &cancel)
        .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("driver did not return through RET")
    );
    let mut changed = artifact()?;
    let at = changed.driver_start as usize - 0x400 + 0x70;
    changed.bytes[at..at + 3].copy_from_slice(&[0xfa, 0, 0xd0]);
    assert!(
        host::Host::artifact(&changed, &song, 0)?
            .run(&native, &cancel)
            .is_err()
    );
    assert!(
        host::Host::source(&bytes, &song)?
            .run(&native, &AtomicBool::new(true))
            .is_err()
    );
    Ok(())
}
