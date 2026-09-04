use anyhow::{Context, bail};
use sha2::{Digest, Sha256};
use zeff_emu_common::save_state::{StateReader, StateWriter};

use super::{PceConsoleWiring, PceHardwareTopology, PceHuCardBoard, PceMachine, PsgRevision};

pub mod tas;

pub const PCE_SAVE_STATE_MAGIC: &[u8; 8] = b"ZBPCE\0\0\0";
const LEGACY_VERSION: u32 = 1;
const PREVIOUS_VERSION: u32 = 2;
pub const PCE_SAVE_STATE_FORMAT_VERSION: u32 = 3;
const CONTENT_HUCARD: u8 = 0;
const CONTENT_CD: u8 = 1;
const MAX_BODY_SIZE: usize = 8 * 1024 * 1024;

#[derive(Clone, Copy)]
pub(super) struct PceStateIdentity {
    pub board: PceHuCardBoard,
    pub topology: PceHardwareTopology,
    pub wiring: PceConsoleWiring,
    pub psg_revision: PsgRevision,
    pub is_cd: bool,
    pub has_arcade_card: bool,
}

pub fn encode_state(machine: &PceMachine) -> anyhow::Result<Vec<u8>> {
    machine.validate_v1_encode_state()?;

    let mut writer = StateWriter::with_capacity(4_500_000);
    writer.write_bytes(PCE_SAVE_STATE_MAGIC);
    writer.write_u32(PCE_SAVE_STATE_FORMAT_VERSION);
    let cdrom2 = machine.devices().cdrom2();
    writer.write_u8(if cdrom2.is_some() {
        CONTENT_CD
    } else {
        CONTENT_HUCARD
    });
    writer.write_bytes(&rom_hash(machine));
    writer.write_u8(board_to_tag(machine.hucard_board()));
    writer.write_u8(topology_to_tag(machine.hardware_topology()));
    writer.write_u8(wiring_to_tag(machine.devices().console_wiring()));
    writer.write_u8(psg_revision_to_tag(machine.devices().psg().revision()));
    writer.write_bool(machine.devices().arcade_card().is_some());
    if let Some(cdrom2) = cdrom2 {
        writer.write_bytes(&cdrom2.disc().content_hash());
    }
    write_section(&mut writer, |section| {
        machine.write_state(section, PCE_SAVE_STATE_FORMAT_VERSION)
    });
    Ok(writer.into_bytes())
}

pub fn decode_state(machine: &mut PceMachine, data: &[u8]) -> anyhow::Result<()> {
    machine.validate_v1_state_target()?;

    let mut reader = StateReader::new(data);
    let mut magic = [0; 8];
    reader.read_exact(&mut magic)?;
    if &magic != PCE_SAVE_STATE_MAGIC {
        bail!("not a valid PC Engine save-state");
    }
    let version = reader.read_u32()?;
    if !matches!(
        version,
        LEGACY_VERSION | PREVIOUS_VERSION | PCE_SAVE_STATE_FORMAT_VERSION
    ) {
        bail!("unsupported PC Engine save-state version {version}");
    }
    let saved_is_cd = match reader.read_u8()? {
        CONTENT_HUCARD => false,
        CONTENT_CD => true,
        tag => bail!("invalid PC Engine content-kind tag in save-state: {tag}"),
    };
    let target_is_cd = machine.devices().cdrom2().is_some();
    if saved_is_cd != target_is_cd {
        bail!("PC Engine save-state media kind does not match the running machine");
    }

    let mut saved_hash = [0; 32];
    reader.read_exact(&mut saved_hash)?;
    if saved_hash != rom_hash(machine) {
        let media = if saved_is_cd { "System Card" } else { "HuCard" };
        bail!("PC Engine save-state belongs to a different {media} ROM");
    }

    let board = tag_to_board(reader.read_u8()?)?;
    if board != machine.hucard_board() {
        bail!("PC Engine save-state HuCard board does not match the loaded cartridge");
    }
    let topology = tag_to_topology(reader.read_u8()?)?;
    if topology != machine.hardware_topology() {
        bail!("PC Engine save-state hardware topology does not match the running machine");
    }
    let wiring = tag_to_wiring(reader.read_u8()?)?;
    if wiring != machine.devices().console_wiring() {
        bail!("PC Engine save-state console wiring does not match the running machine");
    }
    let psg_revision = tag_to_psg_revision(reader.read_u8()?)?;
    if psg_revision != machine.devices().psg().revision() {
        bail!("PC Engine save-state PSG revision does not match the running machine");
    }
    let has_arcade_card = reader.read_bool()?;
    if has_arcade_card != machine.devices().arcade_card().is_some() {
        bail!("PC Engine save-state Arcade Card topology does not match the running machine");
    }
    if saved_is_cd {
        let mut saved_disc_hash = [0; 32];
        reader.read_exact(&mut saved_disc_hash)?;
        if saved_disc_hash
            != machine
                .devices()
                .cdrom2()
                .expect("CD state target has CD hardware")
                .disc()
                .content_hash()
        {
            bail!("PC Engine save-state belongs to different CD media");
        }
    }

    let body = reader.read_vec(MAX_BODY_SIZE)?;
    if !reader.is_exhausted() {
        bail!("PC Engine save-state has unexpected trailing data");
    }
    machine
        .replace_from_state(
            &body,
            PceStateIdentity {
                board,
                topology,
                wiring,
                psg_revision,
                is_cd: saved_is_cd,
                has_arcade_card,
            },
            version,
        )
        .context("invalid PC Engine save-state payload")
}

pub(super) fn write_section(writer: &mut StateWriter, write: impl FnOnce(&mut StateWriter)) {
    writer.write_section(write);
}

pub(super) fn read_section(
    reader: &mut StateReader<'_>,
    max_len: usize,
    label: &str,
    read: impl FnOnce(&mut StateReader<'_>) -> anyhow::Result<()>,
) -> anyhow::Result<()> {
    let data = reader.read_vec(max_len)?;
    let mut section = StateReader::new(&data);
    read(&mut section).with_context(|| format!("invalid PC Engine {label} section"))?;
    if !section.is_exhausted() {
        bail!("PC Engine {label} section has unexpected trailing data");
    }
    Ok(())
}

fn rom_hash(machine: &PceMachine) -> [u8; 32] {
    Sha256::digest(machine.hucard_rom()).into()
}

const fn board_to_tag(board: PceHuCardBoard) -> u8 {
    match board {
        PceHuCardBoard::Plain => 0,
        PceHuCardBoard::Sf2Ce => 1,
        PceHuCardBoard::Populous => 2,
        PceHuCardBoard::SystemCardV1V2 => 3,
        PceHuCardBoard::SystemCardV3 => 4,
    }
}

fn tag_to_board(tag: u8) -> anyhow::Result<PceHuCardBoard> {
    Ok(match tag {
        0 => PceHuCardBoard::Plain,
        1 => PceHuCardBoard::Sf2Ce,
        2 => PceHuCardBoard::Populous,
        3 => PceHuCardBoard::SystemCardV1V2,
        4 => PceHuCardBoard::SystemCardV3,
        _ => bail!("invalid PC Engine HuCard-board tag in save-state: {tag}"),
    })
}

const fn topology_to_tag(topology: PceHardwareTopology) -> u8 {
    match topology {
        PceHardwareTopology::Base => 0,
        PceHardwareTopology::SuperGrafx => 1,
    }
}

fn tag_to_topology(tag: u8) -> anyhow::Result<PceHardwareTopology> {
    Ok(match tag {
        0 => PceHardwareTopology::Base,
        1 => PceHardwareTopology::SuperGrafx,
        _ => bail!("invalid PC Engine hardware-topology tag in save-state: {tag}"),
    })
}

const fn wiring_to_tag(wiring: PceConsoleWiring) -> u8 {
    match wiring {
        PceConsoleWiring::PcEngine => 0,
        PceConsoleWiring::TurboGrafx16 => 1,
    }
}

fn tag_to_wiring(tag: u8) -> anyhow::Result<PceConsoleWiring> {
    Ok(match tag {
        0 => PceConsoleWiring::PcEngine,
        1 => PceConsoleWiring::TurboGrafx16,
        _ => bail!("invalid PC Engine console-wiring tag in save-state: {tag}"),
    })
}

const fn psg_revision_to_tag(revision: PsgRevision) -> u8 {
    match revision {
        PsgRevision::HuC6280 => 0,
        PsgRevision::HuC6280A => 1,
    }
}

fn tag_to_psg_revision(tag: u8) -> anyhow::Result<PsgRevision> {
    Ok(match tag {
        0 => PsgRevision::HuC6280,
        1 => PsgRevision::HuC6280A,
        _ => bail!("invalid PC Engine PSG-revision tag in save-state: {tag}"),
    })
}

#[cfg(test)]
#[path = "save_state/tests.rs"]
mod tests;
