use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use anyhow::{Context, bail};
use zeff_sega8_core::emulator::{DEFAULT_SAMPLE_RATE, Emulator};
use zeff_sega8_core::hardware::bus::CpuAccessTraceEvent;
use zeff_sega8_core::hardware::cartridge::{Sega8MapperKind, Sega8System};
use zeff_sega8_core::hardware::constants::{
    MAPPER_FRAME_CONTROL, MAPPER_SLOT0_BANK, MAPPER_SLOT1_BANK, MAPPER_SLOT2_BANK, SLOT0_END,
    SLOT0_START, SLOT1_END, SLOT1_START, SLOT2_END, SLOT2_START, SMS_Z80_CYCLES_PER_FRAME,
};
use zeff_sega8_core::hardware::cpu::CpuTrap;
use zeff_sega8_core::hardware::vdp::Tms9918Mode;

const DEFAULT_ROM_ROOT: &str = r"Z:\Android\Roms";
const DEFAULT_MAX_INSTRUCTIONS: u64 = 100_000;
const SUPPORTED_ROM_EXTENSIONS: &[&str] = &["sms", "gg", "sg", "sc"];
const ARCHIVE_EXTENSION: &str = "zip";
const FNV1A64_OFFSET_BASIS: u64 = 0xcbf29ce484222325;
const FNV1A64_PRIME: u64 = 0x100000001b3;

#[derive(Clone, Debug)]
struct Config {
    root: PathBuf,
    system_dirs: Vec<String>,
    extensions: Vec<String>,
    limit: Option<usize>,
    max_instructions: u64,
    frames: u64,
    skip_archives: bool,
    show_paths: bool,
    show_root_path: bool,
    issue_report: Option<PathBuf>,
    issue_report_paths: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            root: PathBuf::from(DEFAULT_ROM_ROOT),
            system_dirs: ["."].into_iter().map(str::to_owned).collect(),
            extensions: Vec::new(),
            limit: None,
            max_instructions: DEFAULT_MAX_INSTRUCTIONS,
            frames: 0,
            skip_archives: false,
            show_paths: false,
            show_root_path: false,
            issue_report: None,
            issue_report_paths: false,
        }
    }
}

#[derive(Default)]
struct ProbeReport {
    total_roms: usize,
    systems: BTreeMap<&'static str, SystemStats>,
    issue_candidates: Vec<IssueCandidate>,
}

#[derive(Default)]
struct SystemStats {
    loaded: usize,
    completed_window: usize,
    trapped: usize,
    load_errors: usize,
    read_errors: usize,
    sega_mapper_write_roms: usize,
    codemasters_like_mapper_write_roms: usize,
    other_rom_space_write_roms: usize,
    sega_mapper_kind_roms: usize,
    codemasters_mapper_kind_roms: usize,
    frame_probed_roms: usize,
    frame_completed_roms: usize,
    frame_suspended_roms: usize,
    framebuffer_changed_roms: usize,
    final_non_black_roms: usize,
    final_non_uniform_roms: usize,
    display_enabled_roms: usize,
    vram_nonzero_roms: usize,
    cram_nonzero_roms: usize,
    psg_write_roms: usize,
    final_mode4_roms: usize,
    final_tms_graphics_i_roms: usize,
    final_tms_graphics_ii_roms: usize,
    final_tms_multicolor_roms: usize,
    final_tms_text_roms: usize,
    final_tms_invalid_roms: usize,
    traps: BTreeMap<TrapKey, usize>,
}

#[derive(Default)]
struct RomProbeObservations {
    sega_mapper_write: bool,
    codemasters_like_mapper_write: bool,
    other_rom_space_write: bool,
}

#[derive(Clone, Debug)]
struct IssueCandidate {
    id: usize,
    system: &'static str,
    reasons: Vec<&'static str>,
    frames_completed: Option<u64>,
    requested_frames: Option<u64>,
    final_video_mode: Option<FinalVideoMode>,
    mapper_kind: Option<&'static str>,
    trap: Option<TrapKey>,
    location: Option<String>,
}

#[derive(Clone, Debug, Default)]
struct IssueDetails {
    frames_completed: Option<u64>,
    requested_frames: Option<u64>,
    final_video_mode: Option<FinalVideoMode>,
    mapper_kind: Option<&'static str>,
    trap: Option<TrapKey>,
}

#[derive(Clone, Copy, Debug, Default)]
struct FrameProbeObservations {
    frames_completed: u64,
    framebuffer_changed: bool,
    final_non_black: bool,
    final_non_uniform: bool,
    display_enabled: bool,
    vram_nonzero: bool,
    cram_nonzero: bool,
    psg_written: bool,
    final_video_mode: FinalVideoMode,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum FinalVideoMode {
    #[default]
    Unknown,
    Mode4,
    TmsGraphicsI,
    TmsGraphicsII,
    TmsMulticolor,
    TmsText,
    TmsInvalid,
}

impl FinalVideoMode {
    fn label(self) -> &'static str {
        match self {
            Self::Unknown => "unknown",
            Self::Mode4 => "mode4",
            Self::TmsGraphicsI => "tms_graphics_i",
            Self::TmsGraphicsII => "tms_graphics_ii",
            Self::TmsMulticolor => "tms_multicolor",
            Self::TmsText => "tms_text",
            Self::TmsInvalid => "tms_invalid",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct FramebufferQuality {
    non_black: bool,
    non_uniform: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum TrapKey {
    Opcode(u8),
    Prefixed { prefix: u8, opcode: u8 },
}

impl TrapKey {
    fn from_trap(trap: CpuTrap) -> Self {
        match trap {
            CpuTrap::UnsupportedOpcode { opcode, .. } => Self::Opcode(opcode),
            CpuTrap::UnsupportedPrefixedOpcode { prefix, opcode, .. } => {
                Self::Prefixed { prefix, opcode }
            }
        }
    }
}

fn main() -> anyhow::Result<()> {
    let config = parse_args(env::args().skip(1))?;
    let mut report = ProbeReport::default();

    if config.show_root_path {
        println!("Probing Sega 8-bit ROMs under {}", config.root.display());
    } else {
        println!("Probing Sega 8-bit ROMs under configured root");
    }
    println!(
        "Max instructions per ROM: {}; frame probe: {}; paths are {}",
        config.max_instructions,
        if config.frames == 0 {
            "disabled".to_string()
        } else {
            config.frames.to_string()
        },
        if config.show_paths { "shown" } else { "hidden" }
    );

    for dir in &config.system_dirs {
        if config.limit.is_some_and(|limit| report.total_roms >= limit) {
            break;
        }
        let path = config.root.join(dir);
        if path.is_dir() {
            scan_dir(&path, &config, &mut report).with_context(|| {
                if config.show_paths {
                    format!("failed to scan {}", path.display())
                } else {
                    "failed to scan configured system directory".to_string()
                }
            })?;
        }
    }

    print_report(&report);
    if let Some(path) = &config.issue_report {
        write_issue_report(path, &report)?;
        println!(
            "Issue candidates written: {} -> {} (ROM paths {})",
            report.issue_candidates.len(),
            path.display(),
            if config.issue_report_paths {
                "included"
            } else {
                "hidden"
            }
        );
    }
    Ok(())
}

fn parse_args(args: impl Iterator<Item = String>) -> anyhow::Result<Config> {
    let mut config = Config::default();
    let mut dirs = Vec::new();
    let args = args.collect::<Vec<_>>();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--root" => {
                config.root = next_value(&args, &mut i, "--root")?.into();
            }
            "--dir" => {
                dirs.push(next_value(&args, &mut i, "--dir")?);
            }
            "--extension" | "--ext" => {
                let extension = normalize_extension_arg(&next_value(&args, &mut i, "--extension")?);
                if !SUPPORTED_ROM_EXTENSIONS.contains(&extension.as_str()) {
                    bail!("unsupported extension filter '{extension}'");
                }
                config.extensions.push(extension);
            }
            "--limit" => {
                config.limit = Some(
                    next_value(&args, &mut i, "--limit")?
                        .parse()
                        .context("--limit must be an integer")?,
                );
            }
            "--max-instructions" => {
                config.max_instructions = next_value(&args, &mut i, "--max-instructions")?
                    .parse()
                    .context("--max-instructions must be an integer")?;
            }
            "--frames" => {
                config.frames = next_value(&args, &mut i, "--frames")?
                    .parse()
                    .context("--frames must be an integer")?;
            }
            "--skip-archives" => {
                config.skip_archives = true;
                i += 1;
            }
            "--show-paths" => {
                config.show_paths = true;
                i += 1;
            }
            "--show-root" => {
                config.show_root_path = true;
                i += 1;
            }
            "--issue-report" => {
                config.issue_report = Some(next_value(&args, &mut i, "--issue-report")?.into());
            }
            "--issue-report-paths" => {
                config.issue_report_paths = true;
                i += 1;
            }
            "--help" | "-h" => {
                print_help();
                std::process::exit(0);
            }
            other => bail!("unknown option '{other}'"),
        }
    }

    if !dirs.is_empty() {
        config.system_dirs = dirs;
    }
    Ok(config)
}

fn next_value(args: &[String], index: &mut usize, flag: &str) -> anyhow::Result<String> {
    let value = args
        .get(*index + 1)
        .with_context(|| format!("{flag} requires a value"))?
        .clone();
    *index += 2;
    Ok(value)
}

fn print_help() {
    println!(
        "\
probe_romset [options]

Options:
  --root PATH              ROM root (default: {DEFAULT_ROM_ROOT})
  --dir NAME               Directory under root; repeatable (default: .)
  --extension EXT          ROM extension filter; repeatable (sms, gg, sg, sc)
  --limit N                Stop after N ROM entries
  --max-instructions N     Instruction budget per ROM (default: {DEFAULT_MAX_INSTRUCTIONS})
  --frames N               Run N full frames per ROM and print aggregate frame-quality counts
  --skip-archives          Do not scan ZIP archives
  --show-paths             Print per-ROM status paths; default hides names/paths
  --show-root              Print the configured root path; default hides it
  --issue-report PATH      Write issue-candidate TSV report; default omits ROM paths
  --issue-report-paths     Include ROM paths in --issue-report output
"
    );
}

fn scan_dir(path: &Path, config: &Config, report: &mut ProbeReport) -> anyhow::Result<()> {
    let read_dir = match fs::read_dir(path) {
        Ok(read_dir) => read_dir,
        Err(error) => {
            report.systems.entry("unknown").or_default().read_errors += 1;
            if config.show_paths {
                println!("scan-error {}: {error}", path.display());
            }
            return Ok(());
        }
    };

    let mut entries = Vec::new();
    for entry in read_dir {
        match entry {
            Ok(entry) => entries.push(entry),
            Err(error) => {
                report.systems.entry("unknown").or_default().read_errors += 1;
                if config.show_paths {
                    println!("scan-entry-error {}: {error}", path.display());
                }
            }
        }
    }
    entries.sort_by_key(|entry| entry.path());

    for entry in entries {
        if config.limit.is_some_and(|limit| report.total_roms >= limit) {
            break;
        }

        let path = entry.path();
        let file_type = match entry.file_type() {
            Ok(file_type) => file_type,
            Err(error) => {
                report.systems.entry("unknown").or_default().read_errors += 1;
                if config.show_paths {
                    println!("inspect-error {}: {error}", path.display());
                }
                continue;
            }
        };

        if file_type.is_dir() {
            scan_dir(&path, config, report)?;
            continue;
        }

        if !file_type.is_file() {
            continue;
        }

        let Some(ext) = normalized_extension(&path) else {
            continue;
        };

        if should_probe_extension(&ext, config) {
            report.total_roms += 1;
            probe_file(&path, config, report);
        } else if ext == ARCHIVE_EXTENSION && !config.skip_archives {
            probe_zip(&path, config, report);
        }
    }

    Ok(())
}

fn probe_file(path: &Path, config: &Config, report: &mut ProbeReport) {
    match fs::read(path) {
        Ok(bytes) => probe_rom(&bytes, path, config, report),
        Err(error) => {
            let stats = stats_for_path(report, path);
            stats.read_errors += 1;
            record_issue_candidate(report, path, config, vec!["read_error"], None);
            if config.show_paths {
                println!("read-error {}: {error}", path.display());
            }
        }
    }
}

fn probe_zip(path: &Path, config: &Config, report: &mut ProbeReport) {
    let Ok(file) = fs::File::open(path) else {
        let stats = stats_for_path(report, path);
        stats.read_errors += 1;
        record_issue_candidate(report, path, config, vec!["read_error"], None);
        if config.show_paths {
            println!("read-error {}", path.display());
        }
        return;
    };
    let Ok(mut archive) = zip::ZipArchive::new(file) else {
        let stats = stats_for_path(report, path);
        stats.read_errors += 1;
        record_issue_candidate(report, path, config, vec!["zip_error"], None);
        if config.show_paths {
            println!("zip-error {}", path.display());
        }
        return;
    };

    let mut rom_entries = (0..archive.len())
        .filter_map(|index| {
            let entry = archive.by_index(index).ok()?;
            let name = entry.name().to_string();
            let ext = normalized_extension(Path::new(&name))?;
            should_probe_extension(&ext, config).then_some((index, name))
        })
        .collect::<Vec<_>>();
    rom_entries.sort_by(|a, b| a.1.cmp(&b.1));

    for (index, name) in rom_entries {
        if config.limit.is_some_and(|limit| report.total_roms >= limit) {
            break;
        }

        report.total_roms += 1;
        let virtual_path = path.join(&name);
        let mut bytes = Vec::new();
        match archive.by_index(index) {
            Ok(mut entry) => {
                if let Err(error) = entry.read_to_end(&mut bytes) {
                    let stats = stats_for_path(report, &virtual_path);
                    stats.read_errors += 1;
                    record_issue_candidate(
                        report,
                        &virtual_path,
                        config,
                        vec!["zip_entry_read_error"],
                        None,
                    );
                    if config.show_paths {
                        println!("zip-entry-read-error {}: {error}", virtual_path.display());
                    }
                    continue;
                }
            }
            Err(error) => {
                let stats = stats_for_path(report, &virtual_path);
                stats.read_errors += 1;
                record_issue_candidate(
                    report,
                    &virtual_path,
                    config,
                    vec!["zip_entry_error"],
                    None,
                );
                if config.show_paths {
                    println!("zip-entry-error {}: {error}", virtual_path.display());
                }
                continue;
            }
        }

        probe_rom(&bytes, &virtual_path, config, report);
    }
}

fn probe_rom(bytes: &[u8], path: &Path, config: &Config, report: &mut ProbeReport) {
    let stats = stats_for_path(report, path);
    let mut emulator = match Emulator::new_with_path_hint(bytes, DEFAULT_SAMPLE_RATE, path) {
        Ok(emulator) => emulator,
        Err(error) => {
            stats.load_errors += 1;
            record_issue_candidate(report, path, config, vec!["load_error"], None);
            if config.show_paths {
                println!("load-error {}: {error}", path.display());
            }
            return;
        }
    };
    stats.loaded += 1;
    let mapper_kind = emulator.bus().cartridge.mapper_kind();
    match mapper_kind {
        Sega8MapperKind::Sega => stats.sega_mapper_kind_roms += 1,
        Sega8MapperKind::Codemasters => stats.codemasters_mapper_kind_roms += 1,
    }

    let mut bus_observations = RomProbeObservations::default();
    let mut frame_observations = None;
    if config.frames == 0 {
        run_instruction_probe(
            &mut emulator,
            config.max_instructions,
            &mut bus_observations,
        );
    } else {
        let observations = run_frame_probe(&mut emulator, config.frames, &mut bus_observations);
        record_frame_stats(stats, observations, config.frames);
        frame_observations = Some(observations);
    }

    stats.sega_mapper_write_roms += usize::from(bus_observations.sega_mapper_write);
    stats.codemasters_like_mapper_write_roms +=
        usize::from(bus_observations.codemasters_like_mapper_write);
    stats.other_rom_space_write_roms += usize::from(bus_observations.other_rom_space_write);

    if let Some(trap) = emulator.cpu_trap() {
        let trap_key = TrapKey::from_trap(trap);
        stats.trapped += 1;
        *stats.traps.entry(trap_key.clone()).or_default() += 1;
        record_issue_candidate(
            report,
            path,
            config,
            vec!["cpu_trap"],
            Some(IssueDetails {
                frames_completed: frame_observations
                    .map(|observations| observations.frames_completed),
                requested_frames: (config.frames != 0).then_some(config.frames),
                final_video_mode: frame_observations
                    .map(|observations| observations.final_video_mode),
                mapper_kind: Some(mapper_kind.label()),
                trap: Some(trap_key),
            }),
        );
        if config.show_paths {
            println!("trap {:?}: {}", trap, path.display());
        }
    } else {
        stats.completed_window += 1;
        if let Some(observations) = frame_observations {
            let reasons = frame_issue_reasons(observations, config.frames);
            if !reasons.is_empty() {
                record_issue_candidate(
                    report,
                    path,
                    config,
                    reasons,
                    Some(IssueDetails {
                        frames_completed: Some(observations.frames_completed),
                        requested_frames: Some(config.frames),
                        final_video_mode: Some(observations.final_video_mode),
                        mapper_kind: Some(mapper_kind.label()),
                        trap: None,
                    }),
                );
            }
        }
        if config.show_paths {
            println!("ok {} ({:?})", path.display(), emulator.system());
        }
    }
}

fn run_instruction_probe(
    emulator: &mut Emulator,
    max_instructions: u64,
    bus_observations: &mut RomProbeObservations,
) {
    for _ in 0..max_instructions {
        let (instruction, bus_trace) = emulator.step_instruction_with_bus_trace();
        for event in bus_trace {
            record_observed_bus_event(bus_observations, event);
        }
        if instruction.is_none() || emulator.is_suspended() {
            break;
        }
    }
}

fn run_frame_probe(
    emulator: &mut Emulator,
    frames: u64,
    bus_observations: &mut RomProbeObservations,
) -> FrameProbeObservations {
    let initial_framebuffer = framebuffer_fingerprint(emulator.framebuffer());
    let mut last_framebuffer = initial_framebuffer;
    let mut frame_observations = FrameProbeObservations::default();

    for _ in 0..frames {
        let target_cycles = emulator
            .cpu()
            .cycles()
            .wrapping_add(u64::from(SMS_Z80_CYCLES_PER_FRAME));
        while emulator.cpu().cycles() < target_cycles && !emulator.is_suspended() {
            let (instruction, bus_trace) = emulator.step_instruction_with_bus_trace();
            for event in bus_trace {
                record_observed_bus_event(bus_observations, event);
            }
            if instruction.is_none() || emulator.is_suspended() {
                break;
            }
        }

        if emulator.is_suspended() {
            break;
        }

        emulator.finish_frame();
        frame_observations.frames_completed = frame_observations.frames_completed.wrapping_add(1);
        let framebuffer = framebuffer_fingerprint(emulator.framebuffer());
        if framebuffer != last_framebuffer || framebuffer != initial_framebuffer {
            frame_observations.framebuffer_changed = true;
        }
        last_framebuffer = framebuffer;
    }

    let framebuffer_quality = classify_framebuffer(emulator.framebuffer());
    let vdp = emulator.bus().vdp();
    let mode4 = vdp.mode4_debug_snapshot();
    frame_observations.final_non_black = framebuffer_quality.non_black;
    frame_observations.final_non_uniform = framebuffer_quality.non_uniform;
    frame_observations.display_enabled = vdp.display_enabled();
    frame_observations.vram_nonzero = vdp.vram().iter().any(|&byte| byte != 0);
    frame_observations.cram_nonzero = vdp.cram().iter().any(|&byte| byte != 0);
    frame_observations.psg_written = emulator.bus().apu().write_count() != 0;
    frame_observations.final_video_mode = if mode4.enabled {
        FinalVideoMode::Mode4
    } else {
        match vdp.tms9918_mode() {
            Tms9918Mode::GraphicsI => FinalVideoMode::TmsGraphicsI,
            Tms9918Mode::GraphicsII => FinalVideoMode::TmsGraphicsII,
            Tms9918Mode::Multicolor => FinalVideoMode::TmsMulticolor,
            Tms9918Mode::Text => FinalVideoMode::TmsText,
            Tms9918Mode::Invalid => FinalVideoMode::TmsInvalid,
        }
    };

    frame_observations
}

fn record_frame_stats(
    stats: &mut SystemStats,
    observations: FrameProbeObservations,
    requested_frames: u64,
) {
    stats.frame_probed_roms += 1;
    stats.frame_completed_roms += usize::from(observations.frames_completed == requested_frames);
    stats.frame_suspended_roms += usize::from(observations.frames_completed != requested_frames);
    stats.framebuffer_changed_roms += usize::from(observations.framebuffer_changed);
    stats.final_non_black_roms += usize::from(observations.final_non_black);
    stats.final_non_uniform_roms += usize::from(observations.final_non_uniform);
    stats.display_enabled_roms += usize::from(observations.display_enabled);
    stats.vram_nonzero_roms += usize::from(observations.vram_nonzero);
    stats.cram_nonzero_roms += usize::from(observations.cram_nonzero);
    stats.psg_write_roms += usize::from(observations.psg_written);
    match observations.final_video_mode {
        FinalVideoMode::Unknown => {}
        FinalVideoMode::Mode4 => stats.final_mode4_roms += 1,
        FinalVideoMode::TmsGraphicsI => stats.final_tms_graphics_i_roms += 1,
        FinalVideoMode::TmsGraphicsII => stats.final_tms_graphics_ii_roms += 1,
        FinalVideoMode::TmsMulticolor => stats.final_tms_multicolor_roms += 1,
        FinalVideoMode::TmsText => stats.final_tms_text_roms += 1,
        FinalVideoMode::TmsInvalid => stats.final_tms_invalid_roms += 1,
    }
}

fn frame_issue_reasons(
    observations: FrameProbeObservations,
    requested_frames: u64,
) -> Vec<&'static str> {
    let mut reasons = Vec::new();
    if observations.frames_completed != requested_frames {
        reasons.push("frame_suspended");
    }
    if !observations.framebuffer_changed {
        reasons.push("framebuffer_static");
    }
    if !observations.final_non_black {
        reasons.push("final_black");
    }
    if !observations.final_non_uniform {
        reasons.push("final_uniform");
    }
    if !observations.display_enabled {
        reasons.push("display_disabled");
    }
    if !observations.vram_nonzero {
        reasons.push("vram_zero");
    }
    if !observations.psg_written {
        reasons.push("psg_silent");
    }
    if observations.final_video_mode == FinalVideoMode::TmsInvalid {
        reasons.push("tms_invalid_mode");
    }
    reasons
}

fn record_issue_candidate(
    report: &mut ProbeReport,
    path: &Path,
    config: &Config,
    reasons: Vec<&'static str>,
    details: Option<IssueDetails>,
) {
    if config.issue_report.is_none() || reasons.is_empty() {
        return;
    }

    let details = details.unwrap_or_default();
    let id = report.issue_candidates.len() + 1;
    report.issue_candidates.push(IssueCandidate {
        id,
        system: system_label_from_path(path),
        reasons,
        frames_completed: details.frames_completed,
        requested_frames: details.requested_frames,
        final_video_mode: details.final_video_mode,
        mapper_kind: details.mapper_kind,
        trap: details.trap,
        location: config
            .issue_report_paths
            .then(|| path.display().to_string()),
    });
}

fn write_issue_report(path: &Path, report: &ProbeReport) -> anyhow::Result<()> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }

    let mut out = String::from(
        "id\tsystem\treasons\tframes_completed\trequested_frames\tfinal_video_mode\tmapper\ttrap\tlocation\n",
    );
    for candidate in &report.issue_candidates {
        out.push_str(&candidate.id.to_string());
        out.push('\t');
        out.push_str(candidate.system);
        out.push('\t');
        out.push_str(&escape_tsv(&candidate.reasons.join(",")));
        out.push('\t');
        out.push_str(
            &candidate
                .frames_completed
                .map_or_else(String::new, |value| value.to_string()),
        );
        out.push('\t');
        out.push_str(
            &candidate
                .requested_frames
                .map_or_else(String::new, |value| value.to_string()),
        );
        out.push('\t');
        out.push_str(
            candidate
                .final_video_mode
                .map(FinalVideoMode::label)
                .unwrap_or(""),
        );
        out.push('\t');
        out.push_str(candidate.mapper_kind.unwrap_or(""));
        out.push('\t');
        out.push_str(
            &candidate
                .trap
                .as_ref()
                .map(format_trap_key)
                .unwrap_or_default(),
        );
        out.push('\t');
        out.push_str(&escape_tsv(
            candidate.location.as_deref().unwrap_or("hidden"),
        ));
        out.push('\n');
    }

    fs::write(path, out).with_context(|| format!("failed to write {}", path.display()))
}

fn escape_tsv(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('\t', "\\t")
        .replace('\r', "\\r")
        .replace('\n', "\\n")
}

fn framebuffer_fingerprint(framebuffer: &[u8]) -> u64 {
    framebuffer.iter().fold(FNV1A64_OFFSET_BASIS, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(FNV1A64_PRIME)
    })
}

fn classify_framebuffer(framebuffer: &[u8]) -> FramebufferQuality {
    let mut pixels = framebuffer.chunks_exact(4);
    let Some(first) = pixels.next() else {
        return FramebufferQuality::default();
    };
    let first_rgb = [first[0], first[1], first[2]];
    let mut quality = FramebufferQuality {
        non_black: first_rgb != [0, 0, 0],
        non_uniform: false,
    };

    for pixel in pixels {
        let rgb = [pixel[0], pixel[1], pixel[2]];
        quality.non_black |= rgb != [0, 0, 0];
        quality.non_uniform |= rgb != first_rgb;
        if quality.non_black && quality.non_uniform {
            break;
        }
    }

    quality
}

fn record_observed_bus_event(observations: &mut RomProbeObservations, event: CpuAccessTraceEvent) {
    let CpuAccessTraceEvent::Write { addr, .. } = event else {
        return;
    };

    if is_sega_mapper_register(addr) {
        observations.sega_mapper_write = true;
    } else if is_codemasters_like_mapper_register(addr) {
        observations.codemasters_like_mapper_write = true;
    } else if is_rom_space(addr) {
        observations.other_rom_space_write = true;
    }
}

fn is_sega_mapper_register(addr: u16) -> bool {
    matches!(
        addr,
        MAPPER_FRAME_CONTROL | MAPPER_SLOT0_BANK | MAPPER_SLOT1_BANK | MAPPER_SLOT2_BANK
    )
}

fn is_codemasters_like_mapper_register(addr: u16) -> bool {
    matches!(addr, SLOT0_START | SLOT1_START | SLOT2_START)
}

fn is_rom_space(addr: u16) -> bool {
    matches!(addr, SLOT0_START..=SLOT0_END | SLOT1_START..=SLOT1_END | SLOT2_START..=SLOT2_END)
}

fn stats_for_path<'a>(report: &'a mut ProbeReport, path: &Path) -> &'a mut SystemStats {
    report
        .systems
        .entry(system_label_from_path(path))
        .or_default()
}

fn system_label_from_path(path: &Path) -> &'static str {
    match normalized_extension(path).as_deref() {
        Some("gg") => "gg",
        Some("sg" | "sc") => "sg",
        Some("sms") => "sms",
        _ => "unknown",
    }
}

fn normalized_extension(path: &Path) -> Option<String> {
    path.extension()?
        .to_str()
        .map(|ext| ext.trim_start_matches('.').trim().to_ascii_lowercase())
}

fn normalize_extension_arg(extension: &str) -> String {
    extension
        .trim_start_matches('.')
        .trim()
        .to_ascii_lowercase()
}

fn should_probe_extension(extension: &str, config: &Config) -> bool {
    SUPPORTED_ROM_EXTENSIONS.contains(&extension)
        && (config.extensions.is_empty() || config.extensions.iter().any(|ext| ext == extension))
}

fn print_report(report: &ProbeReport) {
    println!();
    println!("ROM entries probed: {}", report.total_roms);

    for (system, stats) in &report.systems {
        println!();
        println!(
            "{system}: loaded={} trapped={} completed_window={} load_errors={} read_errors={} mapper_kind(sega={} codemasters={}) mapper_writes(sega={} codemasters_like={} other_rom_space={})",
            stats.loaded,
            stats.trapped,
            stats.completed_window,
            stats.load_errors,
            stats.read_errors,
            stats.sega_mapper_kind_roms,
            stats.codemasters_mapper_kind_roms,
            stats.sega_mapper_write_roms,
            stats.codemasters_like_mapper_write_roms,
            stats.other_rom_space_write_roms
        );

        if stats.frame_probed_roms != 0 {
            println!(
                "  frames: probed={} completed={} suspended={} fb_changed={} final_non_black={} final_non_uniform={} display_enabled={} vram_nonzero={} cram_nonzero={} psg_written={}",
                stats.frame_probed_roms,
                stats.frame_completed_roms,
                stats.frame_suspended_roms,
                stats.framebuffer_changed_roms,
                stats.final_non_black_roms,
                stats.final_non_uniform_roms,
                stats.display_enabled_roms,
                stats.vram_nonzero_roms,
                stats.cram_nonzero_roms,
                stats.psg_write_roms
            );
            println!(
                "  final_video_mode: mode4={} tms_g1={} tms_g2={} tms_multicolor={} tms_text={} tms_invalid={}",
                stats.final_mode4_roms,
                stats.final_tms_graphics_i_roms,
                stats.final_tms_graphics_ii_roms,
                stats.final_tms_multicolor_roms,
                stats.final_tms_text_roms,
                stats.final_tms_invalid_roms
            );
        }

        if stats.traps.is_empty() {
            continue;
        }

        let mut traps = stats.traps.iter().collect::<Vec<_>>();
        traps.sort_by(|(left_key, left_count), (right_key, right_count)| {
            right_count
                .cmp(left_count)
                .then_with(|| left_key.cmp(right_key))
        });

        for (trap, count) in traps.into_iter().take(16) {
            println!("  {:>5}  {}", count, format_trap_key(trap));
        }
    }
}

fn format_trap_key(trap: &TrapKey) -> String {
    match trap {
        TrapKey::Opcode(opcode) => format!("opcode 0x{opcode:02X}"),
        TrapKey::Prefixed { prefix, opcode } => {
            format!("prefix 0x{prefix:02X} opcode 0x{opcode:02X}")
        }
    }
}

#[allow(dead_code)]
fn system_label(system: Sega8System) -> &'static str {
    match system {
        Sega8System::MasterSystem => "sms",
        Sega8System::GameGear => "gg",
        Sega8System::Sg1000 => "sg",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mapper_write_observation_classifies_known_registers() {
        let mut observations = RomProbeObservations::default();

        record_observed_bus_event(
            &mut observations,
            CpuAccessTraceEvent::Write {
                addr: MAPPER_SLOT1_BANK,
                old_value: 0,
                new_value: 3,
            },
        );
        assert!(observations.sega_mapper_write);
        assert!(!observations.codemasters_like_mapper_write);

        record_observed_bus_event(
            &mut observations,
            CpuAccessTraceEvent::Write {
                addr: SLOT1_START,
                old_value: 0,
                new_value: 2,
            },
        );
        assert!(observations.codemasters_like_mapper_write);

        record_observed_bus_event(
            &mut observations,
            CpuAccessTraceEvent::Write {
                addr: SLOT1_START + 1,
                old_value: 0,
                new_value: 2,
            },
        );
        assert!(observations.other_rom_space_write);
    }

    #[test]
    fn framebuffer_quality_separates_black_uniform_and_visible_detail() {
        assert_eq!(
            classify_framebuffer(&[[0, 0, 0, 0xFF], [0, 0, 0, 0xFF]].concat()),
            FramebufferQuality {
                non_black: false,
                non_uniform: false,
            }
        );
        assert_eq!(
            classify_framebuffer(&[[1, 2, 3, 0xFF], [1, 2, 3, 0xFF]].concat()),
            FramebufferQuality {
                non_black: true,
                non_uniform: false,
            }
        );
        assert_eq!(
            classify_framebuffer(&[[1, 2, 3, 0xFF], [4, 5, 6, 0xFF]].concat()),
            FramebufferQuality {
                non_black: true,
                non_uniform: true,
            }
        );
    }

    #[test]
    fn extension_filter_accepts_supported_selected_extensions() {
        let mut config = Config::default();

        assert!(should_probe_extension("sms", &config));
        assert!(should_probe_extension("gg", &config));

        config.extensions.push("sms".to_string());

        assert!(should_probe_extension("sms", &config));
        assert!(!should_probe_extension("gg", &config));
        assert!(!should_probe_extension("zip", &config));
    }

    #[test]
    fn frame_issue_reasons_flag_missing_render_and_progress_signals() {
        let reasons = frame_issue_reasons(
            FrameProbeObservations {
                frames_completed: 2,
                framebuffer_changed: false,
                final_non_black: false,
                final_non_uniform: false,
                display_enabled: false,
                vram_nonzero: false,
                cram_nonzero: false,
                psg_written: false,
                final_video_mode: FinalVideoMode::TmsInvalid,
            },
            3,
        );

        assert!(reasons.contains(&"frame_suspended"));
        assert!(reasons.contains(&"framebuffer_static"));
        assert!(reasons.contains(&"final_black"));
        assert!(reasons.contains(&"display_disabled"));
        assert!(reasons.contains(&"vram_zero"));
        assert!(reasons.contains(&"psg_silent"));
        assert!(reasons.contains(&"tms_invalid_mode"));
    }

    #[test]
    fn issue_report_paths_are_hidden_unless_requested() {
        let mut report = ProbeReport::default();
        let mut config = Config {
            issue_report: Some(PathBuf::from("report.tsv")),
            ..Config::default()
        };

        record_issue_candidate(
            &mut report,
            Path::new("private/game.sms"),
            &config,
            vec!["final_black"],
            None,
        );
        config.issue_report_paths = true;
        record_issue_candidate(
            &mut report,
            Path::new("private/game.gg"),
            &config,
            vec!["final_black"],
            None,
        );

        assert_eq!(report.issue_candidates[0].location.as_deref(), None);
        assert_eq!(
            report.issue_candidates[1].location.as_deref(),
            Some("private/game.gg")
        );
    }

    #[test]
    fn tsv_escape_keeps_report_rows_single_line() {
        assert_eq!(escape_tsv("a\tb\nc\\d"), "a\\tb\\nc\\\\d");
    }
}
