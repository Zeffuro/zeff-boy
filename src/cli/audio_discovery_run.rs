use super::*;

pub(super) fn run_request(request: &AudioDiscoveryRequest) -> anyhow::Result<bool> {
    let input = std::sync::Arc::new(load_input(request)?);
    let mut limits = ScanLimits::default();
    if let Some(max_work) = request.max_work {
        limits.max_work = max_work;
    }
    if let Some(max_candidates) = request.max_candidates {
        limits.max_candidates = max_candidates;
    }
    let cancel = AtomicBool::new(false);
    let source_bytes = input
        .cdda
        .as_ref()
        .map_or(input.bytes.len(), |disc| disc.effective_disc_len);
    let manifest = input.analyze(limits, &cancel);
    let status = manifest.scan.status;
    let candidate_count = manifest.scan.song_count();
    let export_request = request
        .export
        .as_ref()
        .map(|export| {
            ensure_distinct_output_path(&export.output_path, &request.input_path)?;
            ensure_distinct_output_path(&export.output_path, &request.output_path)?;
            if export.selection == SongSelection::All {
                return crate::audio_discovery::batch::BatchExportRequest::prepare(
                    &input,
                    &manifest,
                    export.format,
                    |id| export.options_for(id),
                )
                .map(|batch| PreparedExport::Batch(Box::new(batch)));
            }
            let id = select_song(&manifest.scan, export.selection)?;
            SongExportRequest::prepare(
                &input,
                &manifest,
                id,
                export.format,
                export.options_for(id)?,
            )
            .map(|song| PreparedExport::Song(Box::new(song)))
        })
        .transpose()?;
    let relations = request
        .relations
        .as_ref()
        .map(|relations| {
            ensure_distinct_output_path(&relations.output_path, &request.input_path)?;
            ensure_distinct_output_path(&relations.output_path, &request.output_path)?;
            if let Some(export) = &request.export {
                ensure_distinct_output_path(&relations.output_path, &export.output_path)?;
            }
            let id = select_song(&manifest.scan, relations.selection)?;
            let graph = manifest.scan.asset_relations(
                id,
                crate::audio_discovery::relations::GraphLimits::default(),
                &cancel,
            );
            let value = serde_json::json!({
                "schema": "zeff-audio-relations-export/1",
                "analysis_profile": manifest.analysis_profile,
                "source": manifest.source,
                "transforms": manifest.transforms,
                "graph": graph,
            });
            serde_json::to_vec_pretty(&value).context("could not serialize audio relationships")
        })
        .transpose()?;
    manifest.write_new(&request.output_path)?;
    println!(
        "[audio-discovery] status={status:?} candidates={candidate_count} wrote={} source_bytes={}",
        request.output_path.display(),
        source_bytes
    );
    if let (Some(export_request), Some(export)) = (export_request, &request.export) {
        let progress = std::sync::atomic::AtomicU32::new(0);
        match export_request {
            PreparedExport::Song(song) => {
                song.write_new(&export.output_path, &cancel, &progress)?
            }
            PreparedExport::Batch(batch) => {
                let summary = batch.write_new(&export.output_path, &cancel, &progress)?;
                println!(
                    "[audio-export] {} wrote={}",
                    summary.message(),
                    export.output_path.display()
                );
                ensure!(
                    summary.failed == 0,
                    "batch archive was written with {} failed entries; see batch-report.json",
                    summary.failed
                );
            }
        }
        let selection = match export.selection {
            SongSelection::All => "all_songs".to_owned(),
            SongSelection::Offset(offset) => format!("song_offset=0x{offset:08X}"),
            SongSelection::Track(number) => format!("track={number}"),
            SongSelection::Id(id) => format!("song_id={id:?}"),
        };
        println!(
            "[audio-export] format={} {selection} wrote={}",
            export.format.info().id,
            export.output_path.display()
        );
    }
    if let (Some(bytes), Some(relations)) = (relations, &request.relations) {
        use std::io::Seek;
        crate::platform::write_new_file_atomically_validated(
            &relations.output_path,
            &bytes,
            |file| {
                file.rewind()?;
                let _: serde_json::Value = serde_json::from_reader(file)?;
                Ok(())
            },
        )?;
        println!(
            "[audio-relations] wrote={}",
            relations.output_path.display()
        );
    }
    Ok(true)
}

fn select_song(
    report: &crate::audio_discovery::ScanReport,
    selection: SongSelection,
) -> anyhow::Result<SongId> {
    match selection {
        SongSelection::All => anyhow::bail!("a single song is required here"),
        SongSelection::Id(id) => report
            .song(id)
            .map(|_| id)
            .context("requested --audio-song-id was not found in this scan"),
        SongSelection::Offset(offset) => report
            .song_at_offset(offset)
            .context("could not select --audio-song-offset from this scan"),
        SongSelection::Track(number) => report
            .cdda_tracks
            .iter()
            .position(|track| track.number == number)
            .map(SongId::Cdda)
            .context("requested --audio-track was not found among the disc's audio tracks"),
    }
}

enum PreparedExport {
    Song(Box<SongExportRequest>),
    Batch(Box<crate::audio_discovery::batch::BatchExportRequest>),
}
