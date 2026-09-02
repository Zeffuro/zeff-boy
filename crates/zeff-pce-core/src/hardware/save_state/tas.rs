use anyhow::{bail, ensure};
use sha2::{Digest, Sha256};
use zeff_emu_common::save_state::StateReader;

use super::{
    CONTENT_CD, CONTENT_HUCARD, PCE_SAVE_STATE_FORMAT_VERSION, PCE_SAVE_STATE_MAGIC, board_to_tag,
    decode_state, psg_revision_to_tag, topology_to_tag, wiring_to_tag,
};
use crate::hardware::{
    ControllerDevice, ControllerPort, PadButtons, PceCartridgeDescriptor, PceCartridgeHardware,
    PceConsoleWiring, PceControllerMode, PceHardwareTopology, PceHuCardBoard, PceMachine,
    PceVideoActiveBounds, PceVideoRowMetadata, PceVideoSignalBounds, PsgRevision,
    SixButtonExtraButtons, SixButtonPhase, VceFrameLength,
};

pub const TAS_DETERMINISM_ABI_ID: &str = "zeff-pce-determinism-v1";
pub const TAS_STATE_FORMAT_COMPATIBILITY_ID: &str = "zeff-pce-native-v3";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CurrentNativePceTasStateIdentity {
    pub normalized_rom_sha256: [u8; 32],
    pub board: PceHuCardBoard,
    pub topology: PceHardwareTopology,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CurrentNativePceCdTasStateIdentity {
    pub system_card_sha256: [u8; 32],
    pub disc_sha256: [u8; 32],
    pub board: PceHuCardBoard,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CurrentNativePceTasStateProjection {
    pub replay_state_bytes: Vec<u8>,
    pub frame_count: u64,
    pub master_ticks: u64,
    pub vce_line_accumulator: u64,
    pub vdc_pixel_clock_remainder: u8,
    pub vce_line_index: u16,
    pub vce_frame_length: VceFrameLength,
    pub framebuffer: Box<[u8]>,
    pub output_rows: Box<[PceVideoRowMetadata]>,
    pub active_bounds: Option<PceVideoActiveBounds>,
    pub signal_bounds: PceVideoSignalBounds,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CurrentNativePceTasStateInspection {
    pub projection: CurrentNativePceTasStateProjection,
    pub normalized_rom_sha256: [u8; 32],
    pub normalized_rom_len: usize,
    pub board: PceHuCardBoard,
    pub topology: PceHardwareTopology,
    pub wiring: PceConsoleWiring,
    pub psg_revision: PsgRevision,
    pub controller_buttons: PadButtons,
    pub controller_extra_buttons: SixButtonExtraButtons,
    pub controller_six_button_phase: Option<SixButtonPhase>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CurrentNativePceCdTasStateInspection {
    pub projection: CurrentNativePceTasStateProjection,
    pub system_card_sha256: [u8; 32],
    pub system_card_len: usize,
    pub disc_sha256: [u8; 32],
    pub board: PceHuCardBoard,
    pub wiring: PceConsoleWiring,
    pub psg_revision: PsgRevision,
    pub controller_buttons: PadButtons,
    pub arcade_card_enabled: bool,
    pub memory_base_enabled: bool,
}

mod cd;
pub use cd::{
    inspect_current_native_pce_cd_tas_state,
    inspect_current_native_pce_cd_tas_state_for_arcade_card,
    inspect_current_native_pce_cd_tas_state_for_profile,
    inspect_current_native_pce_cd_tas_state_identity,
    inspect_current_native_pce_cd_tas_state_identity_for_arcade_card,
    validate_and_load_current_native_pce_cd_tas_state,
    validate_and_load_current_native_pce_cd_tas_state_for_arcade_card,
    validate_and_load_current_native_pce_cd_tas_state_for_profile,
};

pub fn inspect_current_native_direct_hucard_tas_state(
    machine: &PceMachine,
    data: &[u8],
) -> anyhow::Result<CurrentNativePceTasStateInspection> {
    inspect_current_native_direct_hucard_tas_state_for_board(machine, data, PceHuCardBoard::Plain)
}

pub fn inspect_current_native_direct_hucard_tas_state_for_board(
    machine: &PceMachine,
    data: &[u8],
    board: PceHuCardBoard,
) -> anyhow::Result<CurrentNativePceTasStateInspection> {
    inspect_current_native_direct_hucard_tas_state_for_profile(
        machine,
        data,
        board,
        PceHardwareTopology::Base,
    )
}

pub fn inspect_current_native_direct_hucard_tas_state_for_profile(
    machine: &PceMachine,
    data: &[u8],
    board: PceHuCardBoard,
    topology: PceHardwareTopology,
) -> anyhow::Result<CurrentNativePceTasStateInspection> {
    inspect_current_native_direct_hucard_tas_state_for_profile_and_controller(
        machine,
        data,
        board,
        topology,
        PceControllerMode::TwoButton,
    )
}

pub fn inspect_current_native_direct_hucard_tas_state_for_profile_and_controller(
    machine: &PceMachine,
    data: &[u8],
    board: PceHuCardBoard,
    topology: PceHardwareTopology,
    controller_mode: PceControllerMode,
) -> anyhow::Result<CurrentNativePceTasStateInspection> {
    ensure_current_state(data)?;
    ensure_direct_hucard_machine(machine, board, topology, controller_mode)?;

    let rom_sha256: [u8; 32] = Sha256::digest(machine.hucard_rom()).into();
    let descriptor = PceCartridgeDescriptor::from_sha256(rom_sha256)
        .with_console_wiring(machine.devices().console_wiring())
        .with_hucard_board(board)
        .with_required_hardware(topology_to_cartridge_hardware(topology));
    let mut candidate = PceMachine::with_cartridge_and_controller(
        machine.hucard_rom().to_vec(),
        descriptor,
        match controller_mode {
            PceControllerMode::TwoButton => ControllerPort::two_button(),
            PceControllerMode::SixButton => ControllerPort::six_button(),
            _ => bail!("TAS state requires a supported controller"),
        },
    )?;
    candidate.set_sample_rate(machine.devices().psg().debug_snapshot().sample_rate);
    decode_state(&mut candidate, data)?;
    ensure_direct_hucard_machine(&candidate, board, topology, controller_mode)?;

    let presented = candidate.presented_frame();
    let (controller_buttons, controller_extra_buttons, controller_six_button_phase) =
        match candidate.devices().controller().device() {
            ControllerDevice::TwoButton(pad) => {
                (pad.buttons(), SixButtonExtraButtons::empty(), None)
            }
            ControllerDevice::SixButton(pad) => (
                pad.standard_pad().buttons(),
                pad.extra_buttons(),
                Some(pad.phase()),
            ),
            _ => unreachable!("direct Plain HuCard state validation checked controller topology"),
        };
    Ok(CurrentNativePceTasStateInspection {
        projection: CurrentNativePceTasStateProjection {
            replay_state_bytes: data.to_vec(),
            frame_count: candidate.frame_count(),
            master_ticks: candidate.master_ticks(),
            vce_line_accumulator: candidate.vce_line_accumulator(),
            vdc_pixel_clock_remainder: candidate.vdc_pixel_clock_remainder(),
            vce_line_index: candidate.vce_line_index(),
            vce_frame_length: candidate.vce_frame_length(),
            framebuffer: candidate.framebuffer().into(),
            output_rows: presented.rows().as_slice().into(),
            active_bounds: presented.active_bounds(),
            signal_bounds: presented.signal_bounds(),
        },
        normalized_rom_sha256: rom_sha256,
        normalized_rom_len: candidate.hucard_rom().len(),
        board: candidate.hucard_board(),
        topology: candidate.hardware_topology(),
        wiring: candidate.devices().console_wiring(),
        psg_revision: candidate.devices().psg().revision(),
        controller_buttons,
        controller_extra_buttons,
        controller_six_button_phase,
    })
}

pub fn inspect_current_native_direct_hucard_tas_state_identity(
    data: &[u8],
) -> anyhow::Result<CurrentNativePceTasStateIdentity> {
    inspect_current_native_direct_hucard_tas_state_identity_for_board(data, PceHuCardBoard::Plain)
}

pub fn inspect_current_native_direct_hucard_tas_state_identity_for_board(
    data: &[u8],
    board: PceHuCardBoard,
) -> anyhow::Result<CurrentNativePceTasStateIdentity> {
    ensure_current_state(data)?;
    let mut reader = StateReader::new(data);
    let mut magic = [0; 8];
    reader.read_exact(&mut magic)?;
    let _version = reader.read_u32()?;
    ensure!(
        reader.read_u8()? == CONTENT_HUCARD,
        "TAS state requires direct HuCard media"
    );
    let mut normalized_rom_sha256 = [0; 32];
    reader.read_exact(&mut normalized_rom_sha256)?;
    ensure!(
        reader.read_u8()? == board_to_tag(board)
            && reader.read_u8()? == topology_to_tag(PceHardwareTopology::Base)
            && reader.read_u8()? == wiring_to_tag(PceConsoleWiring::PcEngine)
            && reader.read_u8()? == psg_revision_to_tag(PsgRevision::HuC6280)
            && !reader.read_bool()?,
        "TAS state requires the selected Base PC Engine HuCard hardware"
    );
    Ok(CurrentNativePceTasStateIdentity {
        normalized_rom_sha256,
        board,
        topology: PceHardwareTopology::Base,
    })
}

pub fn inspect_current_native_supported_hucard_tas_state_identity(
    data: &[u8],
) -> anyhow::Result<CurrentNativePceTasStateIdentity> {
    ensure_current_state(data)?;
    let mut reader = StateReader::new(data);
    let mut magic = [0; 8];
    reader.read_exact(&mut magic)?;
    let _version = reader.read_u32()?;
    ensure!(
        reader.read_u8()? == CONTENT_HUCARD,
        "TAS state requires direct HuCard media"
    );
    let mut normalized_rom_sha256 = [0; 32];
    reader.read_exact(&mut normalized_rom_sha256)?;
    let board = match reader.read_u8()? {
        tag if tag == board_to_tag(PceHuCardBoard::Plain) => PceHuCardBoard::Plain,
        tag if tag == board_to_tag(PceHuCardBoard::Sf2Ce) => PceHuCardBoard::Sf2Ce,
        tag if tag == board_to_tag(PceHuCardBoard::Populous) => PceHuCardBoard::Populous,
        _ => bail!("TAS state requires a supported Base PC Engine HuCard board"),
    };
    let topology = match reader.read_u8()? {
        tag if tag == topology_to_tag(PceHardwareTopology::Base) => PceHardwareTopology::Base,
        tag if tag == topology_to_tag(PceHardwareTopology::SuperGrafx)
            && board == PceHuCardBoard::Plain =>
        {
            PceHardwareTopology::SuperGrafx
        }
        _ => bail!("TAS state requires a supported PC Engine HuCard topology"),
    };
    ensure!(
        reader.read_u8()? == wiring_to_tag(PceConsoleWiring::PcEngine)
            && reader.read_u8()? == psg_revision_to_tag(topology_psg_revision(topology))
            && !reader.read_bool()?,
        "TAS state requires supported PC Engine HuCard hardware"
    );
    Ok(CurrentNativePceTasStateIdentity {
        normalized_rom_sha256,
        board,
        topology,
    })
}

pub fn validate_and_load_current_native_direct_hucard_tas_state(
    machine: &mut PceMachine,
    data: &[u8],
) -> anyhow::Result<CurrentNativePceTasStateProjection> {
    validate_and_load_current_native_direct_hucard_tas_state_for_board(
        machine,
        data,
        PceHuCardBoard::Plain,
    )
}

pub fn validate_and_load_current_native_direct_hucard_tas_state_for_board(
    machine: &mut PceMachine,
    data: &[u8],
    board: PceHuCardBoard,
) -> anyhow::Result<CurrentNativePceTasStateProjection> {
    validate_and_load_current_native_direct_hucard_tas_state_for_profile(
        machine,
        data,
        board,
        PceHardwareTopology::Base,
    )
}

pub fn validate_and_load_current_native_direct_hucard_tas_state_for_profile(
    machine: &mut PceMachine,
    data: &[u8],
    board: PceHuCardBoard,
    topology: PceHardwareTopology,
) -> anyhow::Result<CurrentNativePceTasStateProjection> {
    validate_and_load_current_native_direct_hucard_tas_state_for_profile_and_controller(
        machine,
        data,
        board,
        topology,
        PceControllerMode::TwoButton,
    )
}

pub fn validate_and_load_current_native_direct_hucard_tas_state_for_profile_and_controller(
    machine: &mut PceMachine,
    data: &[u8],
    board: PceHuCardBoard,
    topology: PceHardwareTopology,
    controller_mode: PceControllerMode,
) -> anyhow::Result<CurrentNativePceTasStateProjection> {
    let inspection = inspect_current_native_direct_hucard_tas_state_for_profile_and_controller(
        machine,
        data,
        board,
        topology,
        controller_mode,
    )?;
    decode_state(machine, data)?;
    Ok(inspection.projection)
}

fn ensure_current_state(data: &[u8]) -> anyhow::Result<()> {
    if data.len() < 12 || data[..8] != *PCE_SAVE_STATE_MAGIC {
        bail!("TAS requires a native PC Engine save-state");
    }
    let version = u32::from_le_bytes(data[8..12].try_into().expect("length checked"));
    ensure!(
        version == PCE_SAVE_STATE_FORMAT_VERSION,
        "TAS requires PC Engine save-state format {PCE_SAVE_STATE_FORMAT_VERSION}"
    );
    Ok(())
}

fn ensure_direct_hucard_machine(
    machine: &PceMachine,
    board: PceHuCardBoard,
    topology: PceHardwareTopology,
    controller_mode: PceControllerMode,
) -> anyhow::Result<()> {
    ensure!(
        ((topology == PceHardwareTopology::Base
            && matches!(
                board,
                PceHuCardBoard::Plain | PceHuCardBoard::Sf2Ce | PceHuCardBoard::Populous
            ))
            || (topology == PceHardwareTopology::SuperGrafx && board == PceHuCardBoard::Plain))
            && machine.hucard_board() == board
            && machine.hardware_topology() == topology
            && machine.devices().cdrom2().is_none()
            && machine.devices().arcade_card().is_none(),
        "TAS state requires a direct supported Base HuCard machine"
    );
    ensure!(
        matches!(
            (controller_mode, machine.devices().controller().device()),
            (PceControllerMode::TwoButton, ControllerDevice::TwoButton(_))
                | (PceControllerMode::SixButton, ControllerDevice::SixButton(_))
        ) && (controller_mode != PceControllerMode::SixButton
            || (board == PceHuCardBoard::Plain && topology == PceHardwareTopology::Base)),
        "TAS state requires exactly one two-button controller"
    );
    ensure!(
        !machine
            .devices()
            .controller()
            .memory_base128()
            .is_connected(),
        "TAS state requires Memory Base 128 to be disconnected"
    );
    ensure!(
        machine.devices().psg().revision() == topology_psg_revision(topology),
        "TAS state requires the topology's native PSG revision"
    );
    Ok(())
}

fn ensure_pce_cd_machine(
    machine: &PceMachine,
    arcade_card: bool,
    memory_base: bool,
) -> anyhow::Result<()> {
    ensure!(
        machine.hucard_board() == PceHuCardBoard::SystemCardV3
            && machine.hardware_topology() == PceHardwareTopology::Base
            && machine.devices().cdrom2().is_some()
            && machine.devices().arcade_card().is_some() == arcade_card,
        "TAS state requires a Base PC Engine CD machine with Super System Card v3"
    );
    ensure!(
        matches!(
            machine.devices().controller().device(),
            ControllerDevice::TwoButton(_)
        ),
        "TAS state requires exactly one two-button controller"
    );
    ensure!(
        machine
            .devices()
            .controller()
            .memory_base128()
            .is_connected()
            == memory_base,
        "TAS state Memory Base 128 topology does not match the PC Engine CD TAS profile"
    );
    ensure!(
        machine.devices().console_wiring() == PceConsoleWiring::PcEngine
            && machine.devices().psg().revision() == PsgRevision::HuC6280,
        "TAS state requires Base PC Engine wiring and PSG"
    );
    Ok(())
}

const fn topology_to_cartridge_hardware(topology: PceHardwareTopology) -> PceCartridgeHardware {
    match topology {
        PceHardwareTopology::Base => PceCartridgeHardware::Base,
        PceHardwareTopology::SuperGrafx => PceCartridgeHardware::SuperGrafx,
    }
}

const fn topology_psg_revision(topology: PceHardwareTopology) -> PsgRevision {
    match topology {
        PceHardwareTopology::Base => PsgRevision::HuC6280,
        PceHardwareTopology::SuperGrafx => PsgRevision::HuC6280A,
    }
}

#[cfg(test)]
#[path = "tas/tests.rs"]
mod tests;
