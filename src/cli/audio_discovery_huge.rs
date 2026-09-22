use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU32};

use anyhow::{Result, ensure};

use super::audio_discovery_input::{ensure_distinct_output_path, required_path_value};

struct Request {
    output: PathBuf,
    input: PathBuf,
}

pub(in crate::cli) fn run_if_requested(args: &[OsString]) -> Result<bool> {
    let Some(request) = parse(args)? else {
        return Ok(false);
    };
    let bytes = super::audio_discovery_input::read_bounded_cartridge_file(
        &request.input,
        zeff_emu_common::system::System::Gb,
    )?;
    let count = crate::audio_discovery::huge_export::write_new(
        &request.output,
        &bytes,
        &AtomicBool::new(false),
        &AtomicU32::new(0),
    )?;
    println!(
        "[audio-huge] selections={count} wrote={}",
        request.output.display()
    );
    Ok(true)
}

fn parse(args: &[OsString]) -> Result<Option<Request>> {
    if !args.iter().any(|arg| arg == "--audio-huge") {
        return Ok(None);
    }
    ensure!(
        args.first().is_some_and(|arg| arg == "--audio-huge") && args.len() == 3,
        "use --audio-huge NEW.zip SOURCE"
    );
    let output = PathBuf::from(required_path_value(
        args,
        1,
        "hUGE export requires an output ZIP path",
    )?);
    let input = PathBuf::from(required_path_value(
        args,
        2,
        "hUGE export requires an input source",
    )?);
    ensure!(
        output
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("zip")),
        "hUGE output must be a ZIP file"
    );
    ensure_distinct_output_path(&output, &input)?;
    ensure!(
        !output.exists(),
        "hUGE output already exists: {}",
        output.display()
    );
    Ok(Some(Request { output, input }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(values: &[&str]) -> Vec<OsString> {
        values.iter().map(OsString::from).collect()
    }

    #[test]
    fn parser_requires_exact_zip_command() {
        assert!(parse(&args(&["--audio-huge", "out.zip", "source.gb"])).is_ok());
        assert!(parse(&args(&["--audio-huge", "out.gb", "source.gb"])).is_err());
        assert!(parse(&args(&["--audio-huge", "out.zip"])).is_err());
        assert!(parse(&args(&["--audio-huge", "out.zip", "source.gb", "extra"])).is_err());
    }
}
