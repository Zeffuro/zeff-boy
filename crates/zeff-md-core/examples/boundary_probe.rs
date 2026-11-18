#[cfg(any(not(target_arch = "wasm32"), test))]
#[path = "boundary_probe/run.rs"]
mod probe;

#[cfg(test)]
#[path = "boundary_probe/tests.rs"]
mod tests;

#[cfg(not(target_arch = "wasm32"))]
mod native {
    use anyhow::{Context, Result, anyhow, ensure};
    use sha2::{Digest, Sha256};
    use std::io::Write;
    use std::path::Path;
    use zeff_emu_common::time::MachineTiming;
    use zeff_md_core::{CartridgeHeader, ControllerKind, ControllerPort, Emulator, LoadConfig};

    fn hex(bytes: impl AsRef<[u8]>) -> String {
        bytes.as_ref().iter().map(|b| format!("{b:02x}")).collect()
    }

    fn write_new(path: &Path, bytes: &[u8]) -> Result<()> {
        std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)?
            .write_all(bytes)?;
        Ok(())
    }

    pub fn run() -> Result<i32> {
        let args: Vec<_> = std::env::args().skip(1).collect();
        ensure!(
            args.len() == 5,
            "usage: boundary_probe RAW_ROM RAW_STATE TARGET_PC_HEX MAX_BOUNDARIES OUTPUT_DIR"
        );
        let rom = std::fs::read(&args[0]).context("read raw ROM")?;
        let state = std::fs::read(&args[1]).context("read raw state")?;
        let target = u32::from_str_radix(args[2].trim_start_matches("0x"), 16)?;
        ensure!(target & 1 == 0, "target PC must be even");
        let max: u64 = args[3].parse()?;
        ensure!(max > 0, "boundary bound must be positive");
        let output = Path::new(&args[4]);
        std::fs::create_dir(output).context("create fresh output directory")?;
        let rom_hash = hex(Sha256::digest(&rom));
        let restore_hash = hex(Sha256::digest(&state));
        let executable = std::fs::read(std::env::current_exe()?)?;
        let executable_hash = hex(Sha256::digest(executable));
        let region = CartridgeHeader::preferred_region_for_rom(&rom).unwrap_or_default();
        let mut machine = Emulator::new(
            rom,
            LoadConfig {
                region,
                concurrent_sound: true,
            },
        )
        .map_err(|error| anyhow!("load: {error:?}"))?;
        for port in [ControllerPort::One, ControllerPort::Two] {
            machine
                .input_mut()
                .set_controller_kind(port, ControllerKind::SixButton);
        }
        machine.decode_state(&state).context("restore raw state")?;
        let initial_ticks = machine.timing_snapshot().now().get();
        let initial_field = machine.pacing_field_count();
        let initial_pc = machine.registers().pc;
        let report = super::probe::run(&mut machine, max, |m| m.registers().pc == target);
        let code = match &report.stop {
            super::probe::Stop::Reached => 0,
            super::probe::Stop::Limit => 3,
            super::probe::Stop::Fault(_) => 2,
        };
        println!(
            "boundary-probe-v1 rom_sha256={rom_hash} restore_sha256={restore_hash} executable_sha256={executable_hash} region={region:?} concurrent_sound=true controller=SixButton initial_pc={initial_pc:08X} target_pc={target:08X} max_boundaries={max} initial_ticks={initial_ticks} final_ticks={} initial_field={initial_field} final_field={} final_pc={:08X} video_error={:?}",
            machine.timing_snapshot().now().get(),
            machine.pacing_field_count(),
            machine.registers().pc,
            machine.video_error(),
        );
        println!("{report:?}");
        println!("registers={:?}", machine.registers());
        if !matches!(report.stop, super::probe::Stop::Fault(_)) {
            let bytes = machine.encode_state().context("encode final raw state")?;
            write_new(&output.join("final.mdstate"), &bytes)?;
            println!("final_state_sha256={}", hex(Sha256::digest(&bytes)));
        }
        Ok(code)
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    let worker = std::thread::Builder::new()
        .name("boundary-probe".into())
        .stack_size(16 * 1024 * 1024)
        .spawn(native::run);
    let code = match worker {
        Ok(worker) => match worker.join() {
            Ok(Ok(code)) => code,
            Ok(Err(error)) => {
                eprintln!("{error:#}");
                1
            }
            Err(_) => {
                eprintln!("boundary probe panicked");
                1
            }
        },
        Err(error) => {
            eprintln!("start boundary probe: {error}");
            1
        }
    };
    std::process::exit(code);
}

#[cfg(target_arch = "wasm32")]
fn main() {}
