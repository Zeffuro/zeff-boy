use std::fs;
use std::path::Path;

use anyhow::{Context, bail};
use sha2::{Digest, Sha256};

use crate::USER_AGENT;

const COMMIT: &str = "95d8f621ae55cee0d09b91519a8989ae0e64753b";
const BASE_URL: &str = "https://raw.githubusercontent.com/christopherpow/nes-test-roms";

#[derive(Clone, Copy)]
enum Timing {
    Pal,
    Dendy,
}

struct RegionalRom {
    path: &'static str,
    output: &'static str,
    source_sha256: &'static str,
    derived_sha256: &'static str,
    timing: Timing,
}

const ROMS: &[RegionalRom] = &[
    RegionalRom {
        path: "pal_apu_tests/01.len_ctr.nes",
        output: "pal-01.len_ctr.nes",
        source_sha256: "5e4a07738703232dfefce6a26f12da304f333008c60224b27e7fbadf4a7cdc0c",
        derived_sha256: "567cdc921e31cca681c75f1748d01359b4797a0df3382ae8a8c75978597829c0",
        timing: Timing::Pal,
    },
    RegionalRom {
        path: "pal_apu_tests/02.len_table.nes",
        output: "pal-02.len_table.nes",
        source_sha256: "ac5537885469a85e733df1a7a6a0a76a76f157f080c60d04f1128902a45423d4",
        derived_sha256: "432ceb520ab08e9a33017cda46ba960bd0eb1e582fc64c16ba7b10f54da903ff",
        timing: Timing::Pal,
    },
    RegionalRom {
        path: "pal_apu_tests/03.irq_flag.nes",
        output: "pal-03.irq_flag.nes",
        source_sha256: "e0c04111c61d0fc671990c5c3ac6cb7f57082ad687b5e11d380277c7d75e56d1",
        derived_sha256: "012dfcc6e562d58a2f775223646b07a9c7117b6e5ea90a4c4c261465987631bc",
        timing: Timing::Pal,
    },
    RegionalRom {
        path: "pal_apu_tests/04.clock_jitter.nes",
        output: "pal-04.clock_jitter.nes",
        source_sha256: "dc85b14f7ece5e7bd4010b831f5b796debfdf338837c8a29a1d221de8c63776d",
        derived_sha256: "2f56c7fffe824f3dc75f6bbe766f4607838234df443efbebffaa50b3778f5a72",
        timing: Timing::Pal,
    },
    RegionalRom {
        path: "pal_apu_tests/05.len_timing_mode0.nes",
        output: "pal-05.len_timing_mode0.nes",
        source_sha256: "04896f081373f5ab6ce83ce115c5fc0ff823acf831f1499d7d406f4a651e7cbc",
        derived_sha256: "c317ee702662e60e25fe44676dd0842f74d9ee01b94c6dab5b4a6816a5ece583",
        timing: Timing::Pal,
    },
    RegionalRom {
        path: "pal_apu_tests/06.len_timing_mode1.nes",
        output: "pal-06.len_timing_mode1.nes",
        source_sha256: "454b1b6339bd2ea27e3f4e8a8de7e2d95e3afc26940a88255e24a033d42d5a05",
        derived_sha256: "30ad8f77ba4654100348bfe607ca78492226b0e0577646e4fd8f723771f3490e",
        timing: Timing::Pal,
    },
    RegionalRom {
        path: "pal_apu_tests/07.irq_flag_timing.nes",
        output: "pal-07.irq_flag_timing.nes",
        source_sha256: "c91aa1fc7bcb2638f3b07996270eb38c67e8b0fefa1a0db02a34b2e2ffd883c7",
        derived_sha256: "9f7fa42a25ce44e73f660f9dcdcacecada7a49b6bd40d47cdda45f9f2db703a5",
        timing: Timing::Pal,
    },
    RegionalRom {
        path: "pal_apu_tests/08.irq_timing.nes",
        output: "pal-08.irq_timing.nes",
        source_sha256: "dee9e8fac623327b04e8160456362cc1fe4ca0b2c8e3f45eedcb6851ebb00aae",
        derived_sha256: "f555b7d3b7c45c6828f836c686e454703cf8218d6c339a7c7752da18531dcb00",
        timing: Timing::Pal,
    },
    RegionalRom {
        path: "pal_apu_tests/10.len_halt_timing.nes",
        output: "pal-10.len_halt_timing.nes",
        source_sha256: "c41238ed0e7f4044c21fcd14c99b9e4516611adbee5c5f139d3bb95bebebcec9",
        derived_sha256: "bcaeca140bf720b7b2a0a7e99f2eb925f2c1837f933fba5018d0b8180a662889",
        timing: Timing::Pal,
    },
    RegionalRom {
        path: "pal_apu_tests/11.len_reload_timing.nes",
        output: "pal-11.len_reload_timing.nes",
        source_sha256: "1e94a9c0d829378f93b460c2c5f875418490401afd50c30cd05ea22113819909",
        derived_sha256: "334558ab8eea1231419b14aae74d23505a5d07b4ba787f54f83bc114a50b6043",
        timing: Timing::Pal,
    },
    RegionalRom {
        path: "nmi_sync/demo_pal.nes",
        output: "pal-demo_pal.nes",
        source_sha256: "8848fb7f7a20c9acb58a4cdeae2d04a9fd4a33159524cec1f77c802d36861851",
        derived_sha256: "d5c9698c5687903fd8358718ffcb0101bf9024cdf296cf78eea7bb13d7aad98b",
        timing: Timing::Pal,
    },
    RegionalRom {
        path: "240pee/240pee.nes",
        output: "dendy-240pee.nes",
        source_sha256: "228a370b32daacec4c95927aa18243a57be2d45d1d038479ba9d4bb19d05985e",
        derived_sha256: "cf76f0554497e130ac1ace629529a8e94bf7e686f8dde4255fbd0a334d494d68",
        timing: Timing::Dendy,
    },
];

pub(crate) fn build_nes_regional(out_dir: &Path) -> anyhow::Result<()> {
    fs::create_dir_all(out_dir)
        .with_context(|| format!("failed to create {}", out_dir.display()))?;

    for rom in ROMS {
        let url = format!("{BASE_URL}/{COMMIT}/{}", rom.path);
        let mut bytes = ureq::get(&url)
            .header("User-Agent", USER_AGENT)
            .call()
            .map_err(|error| anyhow::anyhow!("HTTP request failed ({url}): {error}"))?
            .into_body()
            .read_to_vec()
            .with_context(|| format!("failed to read HTTP response body from {url}"))?;
        verify_hash(&bytes, rom.source_sha256, rom.path)?;
        apply_timing_header(&mut bytes, rom.timing)?;
        verify_hash(&bytes, rom.derived_sha256, rom.output)?;

        let output = out_dir.join(rom.output);
        fs::write(&output, bytes)
            .with_context(|| format!("failed to write {}", output.display()))?;
        println!("wrote {}", output.display());
    }
    Ok(())
}

fn apply_timing_header(bytes: &mut [u8], timing: Timing) -> anyhow::Result<()> {
    if bytes.len() < 13 {
        bail!(
            "NES header is truncated: expected at least 13 bytes, got {}",
            bytes.len()
        );
    }
    match timing {
        Timing::Pal => bytes[9] |= 0x01,
        Timing::Dendy => {
            bytes[7] = (bytes[7] & 0xf3) | 0x08;
            bytes[12] = (bytes[12] & 0xfc) | 0x03;
        }
    }
    Ok(())
}

fn verify_hash(bytes: &[u8], expected: &str, label: &str) -> anyhow::Result<()> {
    let actual = Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    if actual != expected {
        bail!("unexpected SHA-256 for {label}: expected {expected}, got {actual}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pal_header_sets_the_ines_timing_bit() {
        let mut bytes = [0; 13];
        apply_timing_header(&mut bytes, Timing::Pal).unwrap();
        assert_eq!(bytes[9], 1);
    }

    #[test]
    fn dendy_header_sets_only_dendy_timing_bits() {
        let mut bytes = [0; 13];
        bytes[7] = 0xff;
        bytes[12] = 0xff;
        apply_timing_header(&mut bytes, Timing::Dendy).unwrap();
        assert_eq!(bytes[7], 0xfb);
        assert_eq!(bytes[12], 0xff);
    }

    #[test]
    fn rejects_a_truncated_header() {
        assert!(apply_timing_header(&mut [0; 12], Timing::Pal).is_err());
    }
}
