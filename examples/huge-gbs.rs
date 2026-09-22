use std::{
    io::{Read, Write},
    path::Path,
    sync::atomic::AtomicBool,
};

use anyhow::{Context, Result, ensure};
use zeff_audio_discovery::{ScanStatus, scan};
use zeff_emu_common::system::System;

#[path = "../src/audio_discovery/huge_gbs_validation.rs"]
mod huge_gbs_validation;
#[path = "../src/audio_discovery/huge_validation.rs"]
mod huge_validation;

fn main() -> Result<()> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    ensure!(args.len() == 2, "usage: huge-gbs ROM NEW.gbs");
    let path = Path::new(&args[1]);
    ensure!(!path.exists(), "output already exists");
    let mut bytes = Vec::new();
    std::fs::File::open(&args[0])?
        .take(0x8001)
        .read_to_end(&mut bytes)?;
    let cancel = AtomicBool::new(false);
    let report = scan(System::Gb, &bytes, Default::default(), &cancel);
    ensure!(
        report.status == ScanStatus::Complete && report.huge_songs.len() == 1,
        "requires one supported hUGE catalog selection"
    );
    let (artifact, proof) = huge_gbs_validation::validate(&bytes, &report.huge_songs[0], &cancel)?;
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
    temporary.write_all(&artifact.bytes)?;
    temporary.flush()?;
    temporary
        .persist_noclobber(path)
        .context("cannot publish new GBS")?;
    println!("{}", serde_json::to_string_pretty(&proof)?);
    Ok(())
}
