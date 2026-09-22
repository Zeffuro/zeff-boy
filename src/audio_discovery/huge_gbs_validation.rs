use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::{Result, ensure};
use serde_json::{Value, json};
use zeff_audio_discovery::huge::{catalog::HugeSong, gbs};

#[path = "huge_gbs_validation/control.rs"]
mod control;
#[path = "huge_gbs_validation/host.rs"]
mod host;
#[path = "huge_gbs_validation/native.rs"]
mod native;

pub(crate) fn validate(
    bytes: &[u8],
    song: &HugeSong,
    cancel: &AtomicBool,
) -> Result<(gbs::ExperimentalGbs, Value)> {
    ensure!(
        !cancel.load(Ordering::Relaxed),
        "hUGE GBS validation cancelled"
    );
    let artifact = gbs::build(bytes, song, cancel)?;
    let original = super::huge_validation::validate(
        bytes,
        song.bound.song.descriptor.offset as u16,
        song.validation_frames,
        cancel,
    )?;
    let calls = native::collect(bytes, song, cancel)?;
    let mut reference = host::Host::source(bytes, song)?;
    let expected = reference.run(&calls, cancel)?;
    ensure!(
        expected.iter().any(|sample| sample.abs() > 0.001),
        "GBS reference is silent"
    );
    let mut layouts = Vec::new();
    for fill in [0, 0xff] {
        let mut playback = host::Host::artifact(&artifact, song, fill)?;
        let pcm = playback.run(&calls, cancel)?;
        ensure!(
            pcm.len() == expected.len()
                && pcm
                    .iter()
                    .zip(&expected)
                    .all(|(left, right)| left.to_bits() == right.to_bits()),
            "GBS direct-host PCM differs"
        );
        ensure!(
            playback.writes == reference.writes,
            "GBS direct-host sound timing differs"
        );
        layouts.push(json!({
            "fill": fill,
            "accesses": playback.access_count,
            "recurrence": playback.recurrence,
            "pcm_frames": pcm.len() / 2,
            "pcm_sha256": pcm_hash(&pcm),
        }));
    }
    ensure!(
        !cancel.load(Ordering::Relaxed),
        "hUGE GBS validation cancelled"
    );
    let report = json!({
        "schema": "zeff-huge-gbs-proof/1",
        "passed": true,
        "source_sha256": song.source_sha256,
        "artifact": artifact,
        "gbs_sha256": zeff_firmware::sha256_hex(&artifact.bytes),
        "native_calls": calls.len(),
        "native_call_sha256": zeff_firmware::sha256_hex(&serde_json::to_vec(&calls)?),
        "driver_ram_bytes": 100,
        "normalized_ram_pointer_offsets": (1..=25).step_by(2).collect::<Vec<_>>(),
        "native_normalized_calls_equal": true,
        "host": {
            "init_cycle": 0,
            "first_play_cycle": 70224,
            "play_period_cycles": 70224,
            "initial_registers": "zero",
            "initial_work_ram": "zero",
            "sample_rate": 48000,
            "stack_pointer": 0xfff0,
            "source_veneer_recurrence": reference.recurrence,
            "source_veneer_pcm_sha256": pcm_hash(&expected),
            "sound_writes": reference.writes.len(),
        },
        "layouts": layouts,
        "native_proof": original.report,
        "limitation": "Experimental direct-call GBS ABI proof. PCM equality is to unmodified source routines under the declared host, not to the game's reset/IRQ timeline or external hardware. GBS export remains conditional on this runtime validation.",
    });
    drop((original.pcm, original.isolated));
    Ok((artifact, report))
}

fn pcm_hash(pcm: &[f32]) -> String {
    zeff_firmware::sha256_hex(
        &pcm.iter()
            .flat_map(|value| value.to_bits().to_le_bytes())
            .collect::<Vec<_>>(),
    )
}

#[cfg(test)]
#[path = "huge_gbs_validation/tests.rs"]
mod tests;
