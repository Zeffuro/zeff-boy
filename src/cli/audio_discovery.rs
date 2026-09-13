use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;

use anyhow::{Context, ensure};

use crate::audio_discovery::catalog::SongId;
use crate::audio_discovery::export::SongExportRequest;
use crate::audio_discovery::formats::{BankSelect, SongFormat};
use crate::audio_discovery::natsume::preview::DEFAULT_DURATION_SECONDS;
use crate::audio_discovery::render::{
    MAX_DURATION_SECONDS, MAX_FADE_SECONDS, MAX_LOOP_PASSES, PlaybackGain, RenderOptions,
};
use crate::audio_discovery::{MAX_CANDIDATES, ScanLimits};

#[derive(Debug)]
struct AudioDiscoveryRequest {
    output_path: PathBuf,
    input_path: PathBuf,
    archive_member: Option<String>,
    max_work: Option<u64>,
    max_candidates: Option<u32>,
    export: Option<OfflineExport>,
    relations: Option<RelationsExport>,
    driver_evidence: Option<PathBuf>,
}

#[derive(Debug)]
struct RelationsExport {
    output_path: PathBuf,
    selection: SongSelection,
}

#[derive(Debug)]
struct OfflineExport {
    format: SongFormat,
    output_path: PathBuf,
    selection: SongSelection,
    options: RenderOptions,
    explicit: ExplicitExportSettings,
}

#[derive(Debug, Default)]
struct ExplicitExportSettings {
    sample_rate: bool,
    loops: bool,
    max_seconds: bool,
    fade_seconds: bool,
    midi_channel10: bool,
    bank_select: bool,
    gain: bool,
}

#[path = "audio_discovery_settings.rs"]
mod settings;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SongSelection {
    All,
    Offset(u32),
    Track(u8),
    Id(SongId),
}

pub(crate) fn run_audio_discovery_if_requested() -> anyhow::Result<bool> {
    let Some(request) = parse_audio_discovery_args(std::env::args_os().skip(1))? else {
        return Ok(false);
    };

    run_request(&request)
}

#[path = "audio_discovery_run.rs"]
mod audio_discovery_run;
use audio_discovery_run::run_request;

fn parse_audio_discovery_args(
    args: impl IntoIterator<Item = OsString>,
) -> anyhow::Result<Option<AudioDiscoveryRequest>> {
    let args = args.into_iter().collect::<Vec<_>>();
    if !args.iter().any(|arg| arg == "--audio-discover") {
        if args.iter().any(|arg| {
            arg == "--archive-member"
                || arg == "--audio-scan-work"
                || arg == "--audio-scan-candidates"
                || arg == "--audio-export"
                || arg == "--audio-relations"
                || arg == "--audio-driver-evidence"
                || arg == "--audio-song-offset"
                || arg == "--audio-song"
                || arg == "--audio-song-id"
                || arg == "--audio-all-songs"
                || arg == "--audio-track"
                || arg == "--audio-loops"
                || arg == "--audio-max-seconds"
                || arg == "--audio-fade-seconds"
                || arg == "--audio-midi-channel10"
                || arg == "--audio-bank-select"
                || arg == "--audio-gain"
                || arg == "--audio-sample-rate"
        }) {
            anyhow::bail!("audio scan/export options require --audio-discover");
        }
        return Ok(None);
    }

    let mut request = None;
    let mut archive_member = None;
    let mut max_work = None;
    let mut max_candidates = None;
    let mut export = None;
    let mut relations_path = None;
    let mut driver_evidence = None;
    let mut selection = None;
    let mut options = RenderOptions::default();
    let mut loops_set = false;
    let mut max_seconds_set = false;
    let mut fade_seconds_set = false;
    let mut channel10_set = false;
    let mut bank_select_set = false;
    let mut gain_set = false;
    let mut sample_rate_set = false;
    let mut index = 0;
    while index < args.len() {
        let argument = args[index]
            .to_str()
            .context("audio-discovery arguments must be valid Unicode")?;
        match argument {
            "--audio-driver-evidence" => {
                ensure!(
                    driver_evidence.is_none(),
                    "--audio-driver-evidence may only be specified once"
                );
                driver_evidence = Some(PathBuf::from(required_path_value(
                    &args,
                    index + 1,
                    "--audio-driver-evidence requires a JSON output path",
                )?));
                index += 2;
            }
            "--audio-relations" => {
                ensure!(
                    relations_path.is_none(),
                    "--audio-relations may only be specified once"
                );
                let path = required_path_value(
                    &args,
                    index + 1,
                    "--audio-relations requires a JSON output path",
                )?;
                relations_path = Some(PathBuf::from(path));
                index += 2;
            }
            "--audio-export" => {
                ensure!(
                    export.is_none(),
                    "--audio-export may only be specified once"
                );
                let format = args
                    .get(index + 1)
                    .and_then(|value| value.to_str())
                    .context(
                        "--audio-export requires a registered song format and an output path",
                    )?;
                let format = crate::audio_discovery::formats::parse_song_format(format)?;
                let output = required_path_value(
                    &args,
                    index + 2,
                    "--audio-export requires an output path",
                )?;
                export = Some((format, PathBuf::from(output)));
                index += 3;
            }
            "--audio-loops" => {
                ensure!(!loops_set, "--audio-loops may only be specified once");
                let value = required_audio_u32(&args, index + 1, "--audio-loops")?;
                ensure!(
                    (1..=u32::from(MAX_LOOP_PASSES)).contains(&value),
                    "--audio-loops must be between 1 and {MAX_LOOP_PASSES}"
                );
                options.loops = value as u8;
                loops_set = true;
                index += 2;
            }
            "--audio-max-seconds" => {
                ensure!(
                    !max_seconds_set,
                    "--audio-max-seconds may only be specified once"
                );
                let value = required_audio_u32(&args, index + 1, "--audio-max-seconds")?;
                ensure!(
                    (1..=u32::from(MAX_DURATION_SECONDS)).contains(&value),
                    "--audio-max-seconds must be between 1 and {MAX_DURATION_SECONDS}"
                );
                options.max_seconds = value as u16;
                max_seconds_set = true;
                index += 2;
            }
            "--audio-fade-seconds" => {
                ensure!(
                    !fade_seconds_set,
                    "--audio-fade-seconds may only be specified once"
                );
                let value = required_audio_u32(&args, index + 1, "--audio-fade-seconds")?;
                ensure!(
                    value <= u32::from(MAX_FADE_SECONDS),
                    "--audio-fade-seconds must be between 0 and {MAX_FADE_SECONDS}"
                );
                options.fade_seconds = value as u8;
                fade_seconds_set = true;
                index += 2;
            }
            "--audio-midi-channel10" => {
                ensure!(
                    !channel10_set,
                    "--audio-midi-channel10 may only be specified once"
                );
                options.skip_channel10 = false;
                channel10_set = true;
                index += 1;
            }
            "--audio-bank-select" => {
                ensure!(
                    !bank_select_set,
                    "--audio-bank-select may only be specified once"
                );
                let value = args
                    .get(index + 1)
                    .context("--audio-bank-select requires gs or mma")?
                    .to_str()
                    .context("--audio-bank-select must be valid Unicode")?;
                ensure!(
                    !value.starts_with("--"),
                    "--audio-bank-select requires gs or mma"
                );
                options.bank_select = BankSelect::parse(value)?;
                bank_select_set = true;
                index += 2;
            }
            "--audio-gain" => {
                ensure!(!gain_set, "--audio-gain may only be specified once");
                let value =
                    required_path_value(&args, index + 1, "--audio-gain requires raw or mp2k")?
                        .to_str()
                        .context("--audio-gain must be valid Unicode")?;
                options.playback_gain = PlaybackGain::parse(value)?;
                gain_set = true;
                index += 2;
            }
            "--audio-sample-rate" => {
                ensure!(
                    !sample_rate_set,
                    "--audio-sample-rate may only be specified once"
                );
                options.sample_rate = required_audio_u32(&args, index + 1, "--audio-sample-rate")?;
                sample_rate_set = true;
                index += 2;
            }
            "--audio-song-offset" | "--audio-song" => {
                ensure!(
                    selection.is_none(),
                    "select only one --audio-song-offset, --audio-song-id or --audio-track"
                );
                let value = args
                    .get(index + 1)
                    .and_then(|value| value.to_str())
                    .context("--audio-song-offset requires an effective media offset")?;
                selection = Some(SongSelection::Offset(parse_u32(
                    value,
                    "--audio-song-offset",
                )?));
                index += 2;
            }
            "--audio-all-songs" => {
                ensure!(
                    selection.is_none(),
                    "select either all songs or one song/CD track"
                );
                selection = Some(SongSelection::All);
                index += 1;
            }
            "--audio-song-id" => {
                ensure!(selection.is_none(), "select only one song or CD track");
                let value = required_path_value(
                    &args,
                    index + 1,
                    "--audio-song-id requires engine:index, such as krawall:0",
                )?
                .to_str()
                .context("--audio-song-id must be valid Unicode")?;
                let (engine, item) = value
                    .split_once(':')
                    .context("--audio-song-id requires engine:index, such as krawall:0")?;
                let item = parse_u32(item, "--audio-song-id index")?;
                let id =
                    serde_json::from_value(serde_json::json!({"engine": engine, "index": item}))
                        .context("--audio-song-id has an unknown engine or invalid index")?;
                selection = Some(SongSelection::Id(id));
                index += 2;
            }
            "--audio-track" => {
                ensure!(
                    selection.is_none(),
                    "select only one --audio-song-offset, --audio-song-id or --audio-track"
                );
                let number = required_audio_u32(&args, index + 1, "--audio-track")?;
                ensure!(
                    (1..=99).contains(&number),
                    "--audio-track must be a CD track number between 1 and 99"
                );
                selection = Some(SongSelection::Track(number as u8));
                index += 2;
            }
            "--audio-discover" => {
                ensure!(
                    request.is_none(),
                    "--audio-discover may only be specified once"
                );
                let output_path = required_path_value(
                    &args,
                    index + 1,
                    "--audio-discover requires a report output path and a media input path",
                )?;
                let input_path = required_path_value(
                    &args,
                    index + 2,
                    "--audio-discover requires a report output path and a media input path",
                )?;
                request = Some((PathBuf::from(output_path), PathBuf::from(input_path)));
                index += 3;
            }
            "--archive-member" => {
                ensure!(
                    archive_member.is_none(),
                    "--archive-member may only be specified once"
                );
                let member = args
                    .get(index + 1)
                    .context("--archive-member requires a ZIP member path")?
                    .to_str()
                    .context("--archive-member must be valid Unicode")?;
                ensure!(
                    !member.starts_with("--"),
                    "--archive-member requires a ZIP member path"
                );
                archive_member = Some(normalize_archive_member(member)?);
                index += 2;
            }
            "--audio-scan-work" => {
                ensure!(
                    max_work.is_none(),
                    "--audio-scan-work may only be specified once"
                );
                let value = args
                    .get(index + 1)
                    .context("--audio-scan-work requires an unsigned integer")?
                    .to_str()
                    .context("--audio-scan-work must be valid Unicode")?;
                ensure!(
                    !value.starts_with("--"),
                    "--audio-scan-work requires an unsigned integer"
                );
                max_work = Some(parse_u64(value, "--audio-scan-work")?);
                index += 2;
            }
            "--audio-scan-candidates" => {
                ensure!(
                    max_candidates.is_none(),
                    "--audio-scan-candidates may only be specified once"
                );
                let value = args
                    .get(index + 1)
                    .context("--audio-scan-candidates requires an unsigned integer")?
                    .to_str()
                    .context("--audio-scan-candidates must be valid Unicode")?;
                ensure!(
                    !value.starts_with("--"),
                    "--audio-scan-candidates requires an unsigned integer"
                );
                let parsed = parse_u32(value, "--audio-scan-candidates")?;
                ensure!(
                    (1..=MAX_CANDIDATES).contains(&parsed),
                    "--audio-scan-candidates must be between 1 and {MAX_CANDIDATES}"
                );
                max_candidates = Some(parsed);
                index += 2;
            }
            _ => anyhow::bail!(
                "unexpected audio-discovery argument {argument:?}; use --audio-discover, --archive-member, --audio-scan-work, --audio-scan-candidates, --audio-export, --audio-relations, --audio-driver-evidence, --audio-all-songs, --audio-song-offset, --audio-song-id, --audio-track, --audio-loops, --audio-max-seconds, --audio-fade-seconds, --audio-midi-channel10, --audio-bank-select, --audio-gain, or --audio-sample-rate"
            ),
        }
    }

    let (output_path, input_path) = request.expect("audio-discover flag was found");
    ensure!(
        selection != Some(SongSelection::All) || (export.is_some() && relations_path.is_none()),
        "--audio-all-songs requires --audio-export and cannot be combined with --audio-relations"
    );
    ensure!(
        (export.is_some() || relations_path.is_some()) == selection.is_some(),
        "--audio-export or --audio-relations requires --audio-all-songs or exactly one --audio-song-offset, --audio-song-id or --audio-track"
    );
    ensure!(
        !(loops_set
            || max_seconds_set
            || fade_seconds_set
            || channel10_set
            || bank_select_set
            || gain_set
            || sample_rate_set)
            || export.is_some(),
        "audio export settings require --audio-export"
    );
    ensure!(
        !matches!(selection, Some(SongSelection::Track(_)))
            || !(loops_set
                || max_seconds_set
                || fade_seconds_set
                || channel10_set
                || bank_select_set
                || gain_set
                || sample_rate_set),
        "CD audio export preserves the complete track; synthesis and MIDI settings do not apply to --audio-track"
    );
    ensure!(
        !export.as_ref().is_some_and(|(format, _)| format.is_gsf())
            || !(channel10_set || bank_select_set || gain_set || sample_rate_set),
        "GSF runs the original driver; MIDI, synthesis gain, and output sample-rate options do not apply"
    );
    let export = export.map(|(format, output_path)| OfflineExport {
        format,
        output_path,
        selection: selection.expect("required export song selection"),
        options,
        explicit: ExplicitExportSettings {
            sample_rate: sample_rate_set,
            loops: loops_set,
            max_seconds: max_seconds_set,
            fade_seconds: fade_seconds_set,
            midi_channel10: channel10_set,
            bank_select: bank_select_set,
            gain: gain_set,
        },
    });
    Ok(Some(AudioDiscoveryRequest {
        output_path,
        input_path,
        archive_member,
        max_work,
        max_candidates,
        driver_evidence,
        export,
        relations: relations_path.map(|output_path| RelationsExport {
            output_path,
            selection: selection.expect("required graph song selection"),
        }),
    }))
}

#[path = "audio_discovery_input.rs"]
mod audio_discovery_input;
use audio_discovery_input::*;
#[cfg(test)]
#[path = "audio_discovery_input_tests.rs"]
mod audio_discovery_input_tests;
#[cfg(test)]
#[path = "audio_discovery_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "audio_discovery_export_tests.rs"]
mod export_tests;

#[cfg(test)]
#[path = "audio_discovery_vgm_tests.rs"]
mod vgm_tests;

#[cfg(test)]
#[path = "audio_discovery_rip_tests.rs"]
mod rip_tests;

#[cfg(test)]
#[path = "audio_discovery_native_rip_tests.rs"]
mod native_rip_tests;

#[cfg(test)]
#[path = "audio_discovery_relations_tests.rs"]
mod relations_tests;

#[cfg(test)]
#[path = "audio_discovery_batch_tests.rs"]
mod batch_tests;

#[path = "audio_discovery_drivers.rs"]
mod driver_evidence;
