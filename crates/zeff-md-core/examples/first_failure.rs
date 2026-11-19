#[cfg(not(target_arch = "wasm32"))]
mod native {
    use anyhow::{Context, Result, anyhow, ensure};
    use sha2::{Digest, Sha256};
    use zeff_m68k::bus::Direction;
    use zeff_m68k::timing::TimingFault;
    use zeff_md_core::diagnostic::Capture;
    use zeff_md_core::{
        BusError, CartridgeHeader, ControllerKind, ControllerPort, Emulator, LoadConfig,
        MachineFault, Region,
    };

    fn hex(bytes: impl AsRef<[u8]>) -> String {
        bytes
            .as_ref()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect()
    }

    pub fn run() -> Result<i32> {
        let arguments: Vec<_> = std::env::args().skip(1).collect();
        ensure!(
            (2..=5).contains(&arguments.len()),
            "usage: first_failure RAW_ROM MAX_FIELDS [auto|us|jp|pal] [true|false] [three|six]"
        );
        let max_fields: u64 = arguments[1]
            .parse()
            .context("MAX_FIELDS must be positive")?;
        ensure!(max_fields > 0, "MAX_FIELDS must be positive");
        let rom = std::fs::read(&arguments[0]).context("read raw ROM")?;
        let rom_len = rom.len();
        let rom_sha256 = hex(Sha256::digest(&rom));
        let region = match arguments.get(2).map_or("auto", String::as_str) {
            "auto" => CartridgeHeader::preferred_region_for_rom(&rom).unwrap_or_default(),
            "us" => Region::NtscUs,
            "jp" => Region::NtscJapan,
            "pal" => Region::Pal,
            _ => return Err(anyhow!("region must be auto, us, jp or pal")),
        };
        let concurrent_sound = match arguments.get(3).map_or("true", String::as_str) {
            "true" => true,
            "false" => false,
            _ => return Err(anyhow!("concurrent sound must be true or false")),
        };
        let controller = match arguments.get(4).map_or("six", String::as_str) {
            "six" => ControllerKind::SixButton,
            "three" => ControllerKind::ThreeButton,
            _ => return Err(anyhow!("controller must be three or six")),
        };
        let executable_sha256 = hex(Sha256::digest(
            std::fs::read(std::env::current_exe()?).context("read diagnostic executable")?,
        ));
        let mut machine = Emulator::new(
            rom,
            LoadConfig {
                region,
                concurrent_sound,
            },
        )
        .map_err(|error| anyhow!("load: {error:?}"))?;
        for port in [ControllerPort::One, ControllerPort::Two] {
            machine.input_mut().set_controller_kind(port, controller);
        }
        let capture = Capture::begin().map_err(|error| anyhow!("arm: {error:?}"))?;
        let mut fault = None;
        let mut completed_fields = 0_u64;
        let mut samples = Vec::new();
        let mut audio_hash = Sha256::new();
        let mut sample_count = 0_u64;
        for _ in 0..max_fields {
            if let Err(error) = machine.step_field() {
                fault = Some(error);
                break;
            }
            completed_fields += 1;
            samples.clear();
            machine.drain_audio_samples_into(&mut samples);
            sample_count = sample_count
                .checked_add(samples.len() as u64)
                .context("sample count")?;
            for sample in &samples {
                audio_hash.update(sample.to_bits().to_le_bytes());
            }
        }
        let report = capture
            .finish(&machine)
            .map_err(|error| anyhow!("finish: {error:?}"))?;
        let (status, code) = if !report.complete {
            ("incomplete-capture", 4)
        } else {
            match &fault {
                Some(MachineFault::Main(TimingFault::Bus {
                    direction: Direction::Read,
                    source:
                        BusError::UnsupportedAccess {
                            direction: Direction::Read,
                            ..
                        },
                    ..
                })) => ("unsupported-read", 0),
                Some(_) => ("different-fault", 2),
                None => ("field-bound-exhausted", 3),
            }
        };
        println!(
            "first-failure-v1 status={status} rom_len={rom_len} rom_sha256={rom_sha256} executable_sha256={executable_sha256} region={region:?} concurrent_sound={concurrent_sound} controller={controller:?} completed_fields={completed_fields} max_fields={max_fields} drained_samples={sample_count} drained_audio_sha256={} fault={fault:?}",
            hex(audio_hash.finalize()),
        );
        println!("{report:?}");
        Ok(code)
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    let worker = std::thread::Builder::new()
        .name("first-failure".into())
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
                eprintln!("diagnostic thread panicked");
                1
            }
        },
        Err(error) => {
            eprintln!("start diagnostic thread: {error}");
            1
        }
    };
    std::process::exit(code);
}

#[cfg(target_arch = "wasm32")]
fn main() {}
