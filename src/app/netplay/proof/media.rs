use std::ffi::OsString;
use std::io::Read;
use std::net::SocketAddr;
use std::path::{Component, Path, PathBuf};

use anyhow::{Context, Result, bail, ensure};

use super::write_new;
use crate::emu_backend::{
    ActiveSystem, BackendLoadConfig, EmuBackend, load_backend_from_rom_source,
};

const MAX_BYTES: u64 = 64 * 1024 * 1024;
const USAGE: &str = "usage: --netplay-app-proof NEW_OUTPUT_DIRECTORY [--rom DIRECT_NES_FILE]; set ZEFF_NETPLAY_APP_LAN_ROLE=host|join, FRAMES=24..1000, ADDRESS=numeric-IP:port or INVITATION";

pub(super) struct Options {
    pub(super) root: PathBuf,
    pub(super) role: usize,
    pub(super) frames: u64,
    pub(super) input_delay: zeff_netplay::rollback::InputDelay,
    pub(super) address: SocketAddr,
    pub(super) invitation: Option<String>,
    pub(super) reject_build: bool,
    pub(super) jitter_ms: u64,
    pub(super) paced: bool,
    pub(super) cadence: bool,
    pub(super) fault: Option<String>,
    pub(super) fault_role: usize,
    cartridge: Option<PathBuf>,
}

fn variable(name: &str) -> Result<String> {
    std::env::var(format!("ZEFF_NETPLAY_APP_LAN_{name}"))
        .with_context(|| format!("set ZEFF_NETPLAY_APP_LAN_{name}"))
}

fn bounded(value: &str, min: u64, max: u64, name: &str) -> Result<u64> {
    let number = value
        .parse::<u64>()
        .with_context(|| format!("invalid {name}"))?;
    ensure!((min..=max).contains(&number), "{name} must be {min}..{max}");
    Ok(number)
}

impl Options {
    pub(super) fn parse(arguments: &[OsString]) -> Result<Self> {
        let (root, tail) = arguments.split_first().context(USAGE)?;
        let cartridge = match tail {
            [] => std::env::var_os("ZEFF_NETPLAY_APP_LAN_ROM").map(PathBuf::from),
            [flag, path] if flag == "--rom" => Some(PathBuf::from(path)),
            _ => bail!(USAGE),
        };
        let role = match variable("ROLE")?.as_str() {
            "host" => 0,
            "join" => 1,
            _ => bail!("ROLE must be host or join"),
        };
        let frames = bounded(
            &variable("FRAMES").unwrap_or_else(|_| "24".into()),
            24,
            1000,
            "FRAMES",
        )?;
        let input_delay = zeff_netplay::rollback::InputDelay::new(bounded(
            &variable("INPUT_DELAY")
                .unwrap_or_else(|_| zeff_netplay::rollback::InputDelay::DEFAULT.to_string()),
            zeff_netplay::rollback::InputDelay::MIN,
            zeff_netplay::rollback::InputDelay::MAX,
            "INPUT_DELAY",
        )?)?;
        let jitter_ms = bounded(
            &variable("JITTER_MS").unwrap_or_else(|_| "0".into()),
            0,
            100,
            "JITTER_MS",
        )?;
        let reject_build = match variable("EXPECT").as_deref() {
            Ok("reject-build") => true,
            Ok("") | Err(_) => false,
            Ok(_) => bail!("EXPECT must be reject-build or unset"),
        };
        let paced = match variable("PACED").as_deref() {
            Ok("1") => true,
            Ok("0") | Err(_) => false,
            Ok(_) => bail!("PACED must be 0 or 1"),
        };
        let fault = match variable("FAULT").as_deref() {
            Ok(value @ ("disconnect" | "stall" | "wire")) => Some(value.to_owned()),
            Ok("") | Err(_) => None,
            Ok(_) => bail!("FAULT must be disconnect, stall, wire or unset"),
        };
        let cadence = match variable("CADENCE").as_deref() {
            Ok("1") => true,
            Ok("0") | Err(_) => false,
            Ok(_) => bail!("CADENCE must be 0 or 1"),
        };
        ensure!(
            !cadence || (frames >= 120 && fault.is_none() && !reject_build),
            "cadence requires at least120 frames and no fault/rejection"
        );
        let fault_role = match variable("FAULT_ROLE").as_deref() {
            Ok("host") | Err(_) => 0,
            Ok("join") => 1,
            _ => bail!("FAULT_ROLE must be host or join"),
        };
        ensure!(
            !(reject_build && fault.is_some()),
            "cannot combine rejection and gameplay faults"
        );
        let address = if role == 0 {
            variable("ADDRESS")?
                .parse::<SocketAddr>()
                .context("ADDRESS must be numeric-IP:port")?
        } else {
            "127.0.0.1:0".parse().unwrap()
        };
        let invitation = (role == 1).then(|| variable("INVITATION")).transpose()?;
        if let Some(invitation) = &invitation {
            crate::netplay::connect::validate_invitation(
                invitation,
                zeff_netplay::endpoint::ConnectionScope::TrustedPrivate,
            )?;
        }
        ensure!(
            std::env::var("ZEFF_MUTE_AUDIO").as_deref() == Ok("1"),
            "set ZEFF_MUTE_AUDIO=1 for the headless proof"
        );
        Ok(Self {
            root: PathBuf::from(root),
            role,
            frames,
            input_delay,
            address,
            invitation,
            reject_build,
            jitter_ms,
            paced,
            cadence,
            fault,
            fault_role,
            cartridge,
        })
    }

    pub(super) fn prepare_root(&self) -> Result<()> {
        let config = PathBuf::from(
            std::env::var_os("ZEFF_CONFIG_DIR")
                .context("set isolated ZEFF_CONFIG_DIR inside output root")?,
        );
        self.prepare_root_with_config(&config)
    }

    fn prepare_root_with_config(&self, config: &Path) -> Result<()> {
        ensure!(
            !config.components().any(|part| part == Component::ParentDir),
            "ZEFF_CONFIG_DIR must not contain parent-directory components"
        );
        #[allow(unused_mut)]
        let mut builder = std::fs::DirBuilder::new();
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        builder
            .create(&self.root)
            .context("proof output root must be new with an existing parent")?;
        let root = std::fs::canonicalize(&self.root)?;
        let mut existing = config;
        while !existing.exists() {
            existing = existing
                .parent()
                .context("isolated config has no existing ancestor")?;
        }
        ensure!(
            std::fs::canonicalize(existing)?.starts_with(&root),
            "ZEFF_CONFIG_DIR must be inside the proof root"
        );
        std::fs::create_dir_all(config)?;
        let config = std::fs::canonicalize(config)?;
        ensure!(
            config.starts_with(&root) && config != root,
            "ZEFF_CONFIG_DIR must be a child directory of the proof root"
        );
        Ok(())
    }

    pub(super) fn copy_media(&self) -> Result<PathBuf> {
        let bytes = match &self.cartridge {
            Some(source) => {
                ensure!(
                    source
                        .extension()
                        .is_some_and(|ext| ext.eq_ignore_ascii_case("nes")),
                    "proof requires a direct .nes file"
                );
                read_bounded(source)?
            }
            None => zeff_netplay::fixture::rom(),
        };
        ensure!(
            (16..=MAX_BYTES).contains(&(bytes.len() as u64)) && &bytes[..4] == b"NES\x1a",
            "proof needs an iNES cartridge of 16 bytes..64 MiB"
        );
        let path = self.root.join("cartridge.nes");
        ensure!(
            crate::save_paths::sram_path_for_rom(&path) == path.with_extension("sav"),
            "proof root must not have archive-named ancestors"
        );
        write_new(&path, &bytes)?;
        if let Some(source) = &self.cartridge
            && let Some(save) = optional_save(source)?
        {
            write_new(&path.with_extension("sav"), &save)?;
            write_new(&self.root.join("source-save.bin"), &save)?;
        }
        Ok(path)
    }

    pub(super) fn record_save(&self, save: &Option<Vec<u8>>) -> Result<()> {
        if let Some(bytes) = save {
            write_new(&self.root.join("save-baseline.bin"), bytes)?;
        }
        Ok(())
    }

    pub(super) fn check_save(&self, path: &Path) -> Result<()> {
        ensure!(
            optional_file(&self.root.join("save-baseline.bin"))? == optional_save(path)?,
            "worker shutdown changed session save baseline"
        );
        if let Some(source) = &self.cartridge {
            ensure!(
                read_bounded(source)? == read_bounded(path)?,
                "original cartridge changed"
            );
            ensure!(
                optional_save(source)? == optional_file(&self.root.join("source-save.bin"))?,
                "original save changed"
            );
        }
        Ok(())
    }
}

fn read_bounded(path: &Path) -> Result<Vec<u8>> {
    let file = std::fs::File::open(path).context("opening proof media or save")?;
    ensure!(
        file.metadata()?.is_file(),
        "proof media and saves must be files"
    );
    let mut bytes = Vec::new();
    file.take(MAX_BYTES + 1).read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() as u64 <= MAX_BYTES,
        "proof media or save exceeds 64 MiB"
    );
    Ok(bytes)
}

fn optional_file(path: &Path) -> Result<Option<Vec<u8>>> {
    match read_bounded(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(error)
            if error
                .downcast_ref::<std::io::Error>()
                .is_some_and(|e| e.kind() == std::io::ErrorKind::NotFound) =>
        {
            Ok(None)
        }
        Err(error) => Err(error),
    }
}

pub(super) fn optional_save(path: &Path) -> Result<Option<Vec<u8>>> {
    optional_file(&path.with_extension("sav"))
}

pub(super) fn load(path: &Path) -> Result<EmuBackend> {
    Ok(load_backend_from_rom_source(
        ActiveSystem::Nes,
        path,
        path,
        None,
        BackendLoadConfig {
            apply_mods: false,
            sample_rate: Some(48_000),
            nes_load_battery_sram: true,
            ..BackendLoadConfig::default()
        },
    )?
    .backend)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn proof_bounds_reject_unbounded_or_incomplete_runs() {
        for value in ["0", "23", "1001", "-1", "abc", "18446744073709551616"] {
            assert!(bounded(value, 24, 1000, "FRAMES").is_err());
        }
        assert_eq!(bounded("24", 24, 1000, "FRAMES").unwrap(), 24);
        assert_eq!(bounded("1000", 24, 1000, "FRAMES").unwrap(), 1000);
        assert!(bounded("101", 0, 100, "JITTER_MS").is_err());
    }

    #[test]
    fn optional_save_distinguishes_missing_files_from_failed_reads() {
        let root = crate::test_support::test_directory("app-proof-save-read").unwrap();
        let path = root.path().join("one.nes");
        assert_eq!(optional_save(&path).unwrap(), None);
        std::fs::write(path.with_extension("sav"), [1, 2, 3]).unwrap();
        assert_eq!(optional_save(&path).unwrap(), Some(vec![1, 2, 3]));
        let invalid = root.path().join("directory.nes");
        std::fs::create_dir(invalid.with_extension("sav")).unwrap();
        assert!(optional_save(&invalid).is_err());
    }

    #[test]
    fn config_parent_traversal_is_rejected_before_any_directory_creation() {
        let directory = crate::test_support::test_directory("app-proof-config-traversal").unwrap();
        let root = directory.path().join("new-proof");
        let outside = directory.path().join("outside");
        let options = Options {
            root: root.clone(),
            role: 0,
            frames: 24,
            input_delay: zeff_netplay::rollback::InputDelay::default(),
            address: "127.0.0.1:0".parse().unwrap(),
            invitation: None,
            reject_build: false,
            jitter_ms: 0,
            paced: false,
            cadence: false,
            fault: None,
            fault_role: 0,
            cartridge: None,
        };
        let invalid = root
            .join("missing")
            .join("..")
            .join("..")
            .join("outside/config");
        assert!(options.prepare_root_with_config(&invalid).is_err());
        assert!(!root.exists());
        assert!(!outside.exists());
        options
            .prepare_root_with_config(&root.join("config"))
            .unwrap();
        assert!(root.join("config").is_dir());
        assert!(!outside.exists());
    }
}
