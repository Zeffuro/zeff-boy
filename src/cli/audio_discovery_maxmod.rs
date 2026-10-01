use std::ffi::OsString;
use std::io::Read;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;

use anyhow::{Context, Result, ensure};

use super::audio_discovery_input::{ensure_distinct_output_path, required_path_value};

pub(in crate::cli) fn run_if_requested(args: &[OsString]) -> Result<bool> {
    let Some((output, input)) = parse(args)? else {
        return Ok(false);
    };
    let file = std::fs::File::open(&input).context("failed to open Maxmod source")?;
    let metadata = file.metadata()?;
    let maximum = zeff_audio_discovery::MAX_ROM_BYTES as u64;
    ensure!(metadata.is_file(), "Maxmod source must be a regular file");
    ensure!(
        (1..=maximum).contains(&metadata.len()),
        "Maxmod source exceeds its size limit"
    );
    let mut source = Vec::new();
    file.take(maximum + 1).read_to_end(&mut source)?;
    ensure!(
        source.len() as u64 == metadata.len(),
        "Maxmod source changed while reading"
    );
    crate::audio_discovery::maxmod_export::write_new(&output, &source, &AtomicBool::new(false))?;
    println!("[audio-maxmod-assets] wrote={}", output.display());
    Ok(true)
}

fn parse(args: &[OsString]) -> Result<Option<(PathBuf, PathBuf)>> {
    if !args.iter().any(|arg| arg == "--audio-maxmod-assets") {
        return Ok(None);
    }
    ensure!(
        args.len() == 3 && args[0] == "--audio-maxmod-assets",
        "use --audio-maxmod-assets NEW.zip SOURCE"
    );
    let output = PathBuf::from(required_path_value(
        args,
        1,
        "Maxmod export requires an output ZIP path",
    )?);
    let input = PathBuf::from(required_path_value(
        args,
        2,
        "Maxmod export requires a source",
    )?);
    ensure!(
        output
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("zip")),
        "Maxmod output must be a ZIP file"
    );
    ensure_distinct_output_path(&output, &input)?;
    ensure!(!output.exists(), "Maxmod output already exists");
    Ok(Some((output, input)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parser_requires_exact_asset_command() {
        let args = |values: &[&str]| values.iter().map(OsString::from).collect::<Vec<_>>();
        assert!(
            parse(&args(&[
                "--audio-maxmod-assets",
                "unused.zip",
                "source.gba"
            ]))
            .is_ok()
        );
        for values in [
            vec!["--audio-maxmod-assets", "unused.bin", "source.gba"],
            vec!["--audio-maxmod-assets", "unused.zip"],
            vec!["--audio-maxmod-assets", "unused.zip", "source.gba", "extra"],
            vec!["--audio-discover", "unused.zip", "--audio-maxmod-assets"],
        ] {
            assert!(parse(&args(&values)).is_err());
        }
    }
}
