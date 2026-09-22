use std::collections::BTreeSet;
use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU32};

use anyhow::{Context, Result, ensure};
use zeff_audio_discovery::psglib::FrameRate;

use super::audio_discovery_input::{
    ensure_distinct_output_path, required_audio_u32, required_path_value,
};

struct Request {
    output: PathBuf,
    input: PathBuf,
    offsets: Vec<u32>,
    automatic: bool,
    rate: FrameRate,
}

pub(in crate::cli) fn run_if_requested(args: &[OsString]) -> Result<bool> {
    let Some(request) = parse(args)? else {
        return Ok(false);
    };
    run(&request)?;
    Ok(true)
}

fn parse(args: &[OsString]) -> Result<Option<Request>> {
    if !args.iter().any(|arg| arg == "--audio-psglib") {
        ensure!(
            !args.iter().any(|arg| arg == "--psglib-offset"
                || arg == "--psglib-rate"
                || arg == "--psglib-auto"),
            "PSGlib options require --audio-psglib"
        );
        return Ok(None);
    }
    ensure!(
        args.first().is_some_and(|arg| arg == "--audio-psglib"),
        "use --audio-psglib NEW.zip SOURCE --psglib-rate 50|60 with --psglib-auto or --psglib-offset OFFSET"
    );
    let output = PathBuf::from(required_path_value(
        args,
        1,
        "PSGlib export requires an output ZIP path",
    )?);
    let input = PathBuf::from(required_path_value(
        args,
        2,
        "PSGlib export requires an input source",
    )?);
    let mut rate = None;
    let mut offsets = Vec::new();
    let mut automatic = false;
    let mut unique = BTreeSet::new();
    let mut index = 3;
    while index < args.len() {
        let flag = args[index]
            .to_str()
            .context("PSGlib options must be Unicode")?;
        if flag == "--psglib-auto" {
            ensure!(!automatic, "duplicate --psglib-auto");
            automatic = true;
            index += 1;
            continue;
        }
        let value = required_audio_u32(args, index + 1, flag)?;
        match flag {
            "--psglib-rate" => {
                ensure!(rate.is_none(), "duplicate PSGlib rate");
                rate = Some(match value {
                    50 => FrameRate::Hz50,
                    60 => FrameRate::Hz60,
                    _ => anyhow::bail!("PSGlib projection rate must be 50 or 60 Hz"),
                });
            }
            "--psglib-offset" => {
                ensure!(unique.insert(value), "duplicate PSGlib stream offset");
                ensure!(
                    offsets.len() < 64,
                    "PSGlib export supports at most 64 streams"
                );
                offsets.push(value);
            }
            _ => anyhow::bail!("unsupported PSGlib option: {flag}"),
        }
        index += 2;
    }
    ensure!(
        automatic == offsets.is_empty(),
        "PSGlib export requires either --psglib-auto or explicit offsets"
    );
    let rate = rate.context("PSGlib export requires an explicit 50 or 60 Hz projection rate")?;
    ensure!(
        output
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("zip")),
        "PSGlib output must be a ZIP file"
    );
    ensure_distinct_output_path(&output, &input)?;
    ensure!(!output.exists(), "PSGlib output already exists");
    Ok(Some(Request {
        output,
        input,
        offsets,
        automatic,
        rate,
    }))
}

fn run(request: &Request) -> Result<()> {
    ensure_distinct_output_path(&request.output, &request.input)?;
    ensure!(!request.output.exists(), "PSGlib output already exists");
    let bytes = super::audio_discovery_input::read_bounded_cartridge_file(
        &request.input,
        zeff_emu_common::system::System::Sms,
    )?;
    let cancel = AtomicBool::new(false);
    let selection = if request.automatic {
        let system = super::audio_discovery_input::cartridge_system(&request.input)
            .filter(|system| {
                matches!(
                    system,
                    zeff_emu_common::system::System::Sms | zeff_emu_common::system::System::Gg
                )
            })
            .context("automatic PSGlib discovery requires an SMS or Game Gear cartridge input")?;
        crate::audio_discovery::psglib_export::Selection::Automatic(system)
    } else {
        crate::audio_discovery::psglib_export::Selection::Offsets(&request.offsets)
    };
    let count = crate::audio_discovery::psglib_export::write_new(
        &request.output,
        &bytes,
        selection,
        request.rate,
        &cancel,
        &AtomicU32::new(0),
    )?;
    println!(
        "[audio-psglib] streams={} rate={} wrote={}",
        count,
        request.rate.hz(),
        request.output.display()
    );
    Ok(())
}

#[cfg(test)]
#[path = "audio_discovery_psglib_tests.rs"]
mod tests;
