use std::io::Read;
use std::sync::atomic::AtomicBool;

use anyhow::{Context, Result, ensure};

#[path = "../src/audio_discovery/huge_validation.rs"]
mod validation;

fn main() -> Result<()> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    ensure!(
        args.len() == 3,
        "usage: huge-closure ROM DESCRIPTOR_OFFSET FRAMES"
    );
    let text = args[1].to_str().context("descriptor must be text")?;
    let descriptor: u16 = match text.strip_prefix("0x") {
        Some(hex) => u16::from_str_radix(hex, 16)?,
        None => text.parse()?,
    };
    let frames: u32 = args[2].to_str().context("frames must be text")?.parse()?;
    let mut bytes = Vec::new();
    std::fs::File::open(&args[0])?
        .take(0x8001)
        .read_to_end(&mut bytes)?;
    let result = validation::validate(&bytes, descriptor, frames, &AtomicBool::new(false))?;
    println!("{}", serde_json::to_string_pretty(&result.report)?);
    drop((result.pcm, result.isolated));
    Ok(())
}
