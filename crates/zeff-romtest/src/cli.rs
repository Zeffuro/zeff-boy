use std::ffi::OsString;
use std::path::PathBuf;

use anyhow::{Context, bail};

use crate::model::{Core, Tier};
use crate::{
    DEFAULT_BASELINE_PATH, DEFAULT_MANIFEST_DIR, DEFAULT_SOURCE_CACHE_DIR, DEFAULT_SOURCES_PATH,
};

#[derive(Debug)]
pub(crate) struct Cli {
    pub(crate) command: CommandKind,
    pub(crate) manifest_dir: PathBuf,
    pub(crate) sources_path: PathBuf,
    pub(crate) source_cache_dir: PathBuf,
    pub(crate) filter: TestFilter,
    pub(crate) include_games: bool,
    pub(crate) allow_missing: bool,
    pub(crate) dry_run: bool,
    pub(crate) zeff_boy: Option<PathBuf>,
    pub(crate) report_json: Option<PathBuf>,
    pub(crate) report_md: Option<PathBuf>,
    pub(crate) report_junit: Option<PathBuf>,
    pub(crate) report_baseline: Option<PathBuf>,
    pub(crate) baseline: Option<PathBuf>,
    pub(crate) actual_json: Option<PathBuf>,
    pub(crate) compat_rom_dir: Option<PathBuf>,
    pub(crate) compat_output: Option<PathBuf>,
    pub(crate) compat_max_frames: u64,
    pub(crate) compat_limit: Option<usize>,
    pub(crate) compat_name_matches: Vec<String>,
    pub(crate) fixture: Option<crate::fixtures::FixtureKind>,
    pub(crate) fixture_out_dir: Option<PathBuf>,
}

impl Cli {
    pub(crate) fn parse(args: impl IntoIterator<Item = OsString>) -> anyhow::Result<Self> {
        let mut args = args.into_iter();
        let command = match args.next().as_deref().and_then(|s| s.to_str()) {
            None | Some("help") | Some("--help") | Some("-h") => CommandKind::Help,
            Some("list") => CommandKind::List,
            Some("check") => CommandKind::Check,
            Some("prepare") => CommandKind::Prepare,
            Some("fetch") => CommandKind::Fetch,
            Some("build-fixture") => CommandKind::BuildFixture,
            Some("build-nes-regional") => CommandKind::BuildNesRegional,
            Some("audit-tooling") => CommandKind::AuditTooling,
            Some("audit-source-size") => CommandKind::AuditSourceSize,
            Some("run") => CommandKind::Run,
            Some("compare") => CommandKind::Compare,
            Some("generate-compat") => CommandKind::GenerateCompat,
            Some("summary") | Some("status") => CommandKind::Summary,
            Some(other) => bail!("unknown command '{other}'"),
        };

        let mut cli = Self {
            command,
            manifest_dir: PathBuf::from(DEFAULT_MANIFEST_DIR),
            sources_path: PathBuf::from(DEFAULT_SOURCES_PATH),
            source_cache_dir: PathBuf::from(DEFAULT_SOURCE_CACHE_DIR),
            filter: TestFilter::default(),
            include_games: false,
            allow_missing: false,
            dry_run: false,
            zeff_boy: None,
            report_json: None,
            report_md: None,
            report_junit: None,
            report_baseline: None,
            baseline: None,
            actual_json: None,
            compat_rom_dir: None,
            compat_output: None,
            compat_max_frames: 3600,
            compat_limit: None,
            compat_name_matches: Vec::new(),
            fixture: None,
            fixture_out_dir: None,
        };

        let rest: Vec<OsString> = args.collect();
        let mut i = 0;
        while i < rest.len() {
            let flag = rest[i].to_string_lossy();
            match flag.as_ref() {
                "--manifest-dir" => {
                    cli.manifest_dir = next_path(&rest, &mut i, "--manifest-dir")?;
                }
                "--sources" => {
                    cli.sources_path = next_path(&rest, &mut i, "--sources")?;
                }
                "--source-cache-dir" => {
                    cli.source_cache_dir = next_path(&rest, &mut i, "--source-cache-dir")?;
                }
                "--core" => {
                    cli.filter.core = Some(next_value(&rest, &mut i, "--core")?.parse()?);
                }
                "--tier" => {
                    cli.filter.tier = Some(next_value(&rest, &mut i, "--tier")?.parse()?);
                }
                "--exclude-tier" => {
                    cli.filter
                        .exclude_tiers
                        .push(next_value(&rest, &mut i, "--exclude-tier")?.parse()?);
                }
                "--id" => {
                    cli.filter.id_contains = Some(next_value(&rest, &mut i, "--id")?);
                }
                "--tag" => {
                    cli.filter.tag = Some(next_value(&rest, &mut i, "--tag")?);
                }
                "--include-games" => {
                    cli.include_games = true;
                    i += 1;
                }
                "--allow-missing" => {
                    cli.allow_missing = true;
                    i += 1;
                }
                "--dry-run" => {
                    cli.dry_run = true;
                    i += 1;
                }
                "--zeff-boy" => {
                    cli.zeff_boy = Some(next_path(&rest, &mut i, "--zeff-boy")?);
                }
                "--report-json" => {
                    cli.report_json = Some(next_path(&rest, &mut i, "--report-json")?);
                }
                "--report-md" => {
                    cli.report_md = Some(next_path(&rest, &mut i, "--report-md")?);
                }
                "--report-junit" => {
                    cli.report_junit = Some(next_path(&rest, &mut i, "--report-junit")?);
                }
                "--report-baseline" => {
                    cli.report_baseline = Some(next_path(&rest, &mut i, "--report-baseline")?);
                }
                "--baseline" => {
                    cli.baseline = Some(next_path(&rest, &mut i, "--baseline")?);
                }
                "--actual-json" => {
                    cli.actual_json = Some(next_path(&rest, &mut i, "--actual-json")?);
                }
                "--rom-dir" => {
                    cli.compat_rom_dir = Some(next_path(&rest, &mut i, "--rom-dir")?);
                }
                "--output" => {
                    cli.compat_output = Some(next_path(&rest, &mut i, "--output")?);
                }
                "--max-frames" => {
                    cli.compat_max_frames = next_value(&rest, &mut i, "--max-frames")?
                        .parse()
                        .context("--max-frames must be an integer")?;
                }
                "--limit" => {
                    cli.compat_limit = Some(
                        next_value(&rest, &mut i, "--limit")?
                            .parse()
                            .context("--limit must be an integer")?,
                    );
                }
                "--name-match" => {
                    cli.compat_name_matches
                        .push(next_value(&rest, &mut i, "--name-match")?);
                }
                "--fixture" => {
                    cli.fixture = Some(crate::fixtures::FixtureKind::parse(&next_value(
                        &rest,
                        &mut i,
                        "--fixture",
                    )?)?);
                }
                "--out-dir" => {
                    cli.fixture_out_dir = Some(next_path(&rest, &mut i, "--out-dir")?);
                }
                "--help" | "-h" => {
                    cli.command = CommandKind::Help;
                    i += 1;
                }
                _ => bail!("unknown option '{flag}'"),
            }
        }

        Ok(cli)
    }
}

fn next_value(args: &[OsString], index: &mut usize, flag: &str) -> anyhow::Result<String> {
    let value = args
        .get(*index + 1)
        .with_context(|| format!("{flag} requires a value"))?;
    *index += 2;
    Ok(value.to_string_lossy().into_owned())
}

fn next_path(args: &[OsString], index: &mut usize, flag: &str) -> anyhow::Result<PathBuf> {
    Ok(PathBuf::from(next_value(args, index, flag)?))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CommandKind {
    Help,
    List,
    Check,
    Prepare,
    Fetch,
    BuildFixture,
    BuildNesRegional,
    AuditTooling,
    AuditSourceSize,
    Run,
    Compare,
    GenerateCompat,
    Summary,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct TestFilter {
    pub(crate) core: Option<Core>,
    pub(crate) tier: Option<Tier>,
    pub(crate) exclude_tiers: Vec<Tier>,
    pub(crate) id_contains: Option<String>,
    pub(crate) tag: Option<String>,
}

pub(crate) fn print_help() {
    let core_values = Core::supported_values();
    println!(
        "\
zeff-romtest <command> [options]

Commands:
  list                 List selected manifest entries
  check                Validate manifests and repository policy
  prepare              Populate rom-tests/cache from known local legacy paths
  fetch                Download pinned source archives and extract selected test ROMs
  build-fixture        Build a platform-neutral generated ROM fixture
  build-nes-regional   Derive pinned PAL and Dendy NES acceptance ROMs
  audit-tooling        Enforce script ownership and fast/slow CI boundaries
  audit-source-size    Enforce the first-party Rust source-size ratchet
  run                  Run selected tests through Zeff Boy headless CLI
  compare              Compare a run JSON report against a baseline JSON report
  generate-compat      Generate an ignored local game compatibility manifest
  summary              Print status/coverage/suite tables from an existing report
  status               Alias for summary

Options:
  --manifest-dir PATH  Manifest file or directory (default: {DEFAULT_MANIFEST_DIR})
  --sources PATH       Source catalog path (default: {DEFAULT_SOURCES_PATH})
  --source-cache-dir PATH
                        Download cache for source archives (default: {DEFAULT_SOURCE_CACHE_DIR})
  --core {core_values} Filter by core
  --tier TIER          Filter by tier: smoke|accuracy|visual|local|compat
  --exclude-tier TIER  Exclude a tier; may be repeated
  --id TEXT            Filter by id substring
  --tag TEXT           Filter by tag
  --include-games      Include game_rom entries in selection
  --allow-missing      Treat missing selected ROM files as skipped
  --dry-run            Print commands without running them
  --zeff-boy PATH      Use a prebuilt zeff-boy executable instead of cargo run -p zeff-boy
  --report-json PATH   Write JSON report
  --report-md PATH     Write Markdown table report
  --report-junit PATH  Write JUnit XML report
  --report-baseline PATH
                        Write deterministic baseline JSON from a run
  --baseline PATH      Baseline JSON path for compare
                        or summary (default: {DEFAULT_BASELINE_PATH})
  --actual-json PATH   Actual run JSON path for compare
                        or summary
  --rom-dir PATH       ROM directory for generate-compat
  --output PATH        Output manifest for generate-compat
                        (default: rom-tests/manifests/compat-games/local-generated.toml)
  --max-frames N       Max frames per generated compatibility entry (default: 3600)
  --limit N            Limit generated compatibility entries
  --name-match TEXT    Only include ROM filenames containing TEXT; may be repeated
  --fixture NAME       Fixture for build-fixture: sega8|pce-cd-adpcm-irq|pce-vdc-fetch-contention
  --out-dir PATH       Output directory for build-fixture
"
    );
}
