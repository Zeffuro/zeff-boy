use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU32};

use anyhow::{Result, ensure};
use serde_json::json;
use zeff_audio_discovery::psglib::{self, FrameRate};

pub(crate) enum Selection<'a> {
    Offsets(&'a [u32]),
    Automatic(zeff_emu_common::system::System),
}

pub(crate) fn write_new(
    path: &Path,
    source: &[u8],
    selection: Selection<'_>,
    rate: FrameRate,
    cancel: &AtomicBool,
    progress: &AtomicU32,
) -> Result<usize> {
    let (offsets, selection, limitation) = match selection {
        Selection::Offsets(offsets) => {
            ensure!(
                !offsets.is_empty() && offsets.len() <= 64,
                "PSGlib export requires 1..=64 streams"
            );
            (
                offsets.to_vec(),
                json!({"kind": "explicit_offsets", "offsets": offsets}),
                "Offsets are explicitly supplied; this operation does not discover or authenticate a PSGlib driver, original bank mapping, song table or complete soundtrack.",
            )
        }
        Selection::Automatic(system) => {
            let report = psglib::discover(
                source,
                zeff_audio_discovery::ScanLimits {
                    max_candidates: 64,
                    ..Default::default()
                },
                cancel,
            )
            .map_err(|stop| anyhow::anyhow!("PSGlib discovery stopped: {stop:?}"))?;
            let offsets = report
                .bound
                .iter()
                .map(|stream| stream.offset)
                .collect::<std::collections::BTreeSet<_>>()
                .into_iter()
                .collect::<Vec<_>>();
            (
                offsets,
                json!({"kind": "static_calls", "system": system, "mapping": "reset_initial_identity_32k", "root_bits": {"reset": 1, "irq": 2, "nmi": 4}, "discovery": report}),
                "Static driver and call-site evidence selects bounded streams assuming CPU addresses equal source offsets in an initial image of at most 32 KiB. It does not prove executed calls, runtime bank mapping, original playback state, song roles or complete soundtrack coverage.",
            )
        }
    };
    ensure!(
        offsets
            .iter()
            .collect::<std::collections::BTreeSet<_>>()
            .len()
            == offsets.len(),
        "duplicate PSGlib stream offset"
    );
    let mut bundle = super::bundle::Bundle::new();
    let mut songs = Vec::new();
    for &offset in &offsets {
        let decoded = psglib::decode(source, offset, cancel)?;
        let vgm = psglib::export_vgm(source, offset, rate, cancel)?;
        let prefix = format!("stream-{offset:08x}");
        let vgm_path = format!("{prefix}/music.vgm");
        let events_path = format!("{prefix}/frames.json");
        let events = serde_json::to_vec_pretty(&decoded)?;
        bundle.add(&vgm_path, &vgm)?;
        bundle.add(&events_path, &events)?;
        let mut spans = Vec::new();
        for span in &decoded.spans {
            let data = &source[span.offset as usize..(span.offset + span.byte_len) as usize];
            let name = format!("{prefix}/source-{:08x}.bin", span.offset);
            bundle.add(&name, data)?;
            spans.push(
                json!({"span": span, "path": name, "sha256": zeff_firmware::sha256_hex(data)}),
            );
        }
        songs.push(json!({
            "offset": offset, "frames": decoded.frames, "writes": decoded.writes.len(),
            "vgm": {"path": vgm_path, "sha256": zeff_firmware::sha256_hex(&vgm)},
            "events": {"path": events_path, "sha256": zeff_firmware::sha256_hex(&events)},
            "source_spans": spans,
        }));
    }
    let manifest = json!({
        "schema": "zeff-psglib-stream-export/2",
        "source": {"byte_len": source.len(), "sha256": zeff_firmware::sha256_hex(source)},
        "format_reference_revision": psglib::SOURCE_REVISION,
        "projection": {"frame_hz": rate.hz(), "chip_clock_hz": 3579545, "model": "sega_psg", "passes": 1},
        "selection": selection,
        "streams": songs,
        "limitations": [
            limitation,
            "Each stream is interpreted once with no SFX, zero volume attenuation and stop mutes. Loop markers are retained but not repeated; nested substrings and control markers within substrings are unsupported.",
            "The chosen 50/60 Hz cadence and Sega PSG clock are a semantic projection, not original-game timing or PCM equivalence. VGM writes within each frame share a timestamp.",
            "Source spans contain visited bytes within a 16 KiB stream window; unrelated and unvisited bytes are not included."
        ],
    });
    bundle.add("manifest.json", &serde_json::to_vec_pretty(&manifest)?)?;
    super::assets::publish_bytes(path, &bundle.finish()?, cancel, progress)?;
    Ok(offsets.len())
}
