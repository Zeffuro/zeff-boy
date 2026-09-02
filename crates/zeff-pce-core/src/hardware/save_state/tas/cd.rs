use super::*;

pub fn inspect_current_native_pce_cd_tas_state(
    machine: &PceMachine,
    data: &[u8],
) -> anyhow::Result<CurrentNativePceCdTasStateInspection> {
    inspect_current_native_pce_cd_tas_state_for_arcade_card(machine, data, false)
}

pub fn inspect_current_native_pce_cd_tas_state_for_arcade_card(
    machine: &PceMachine,
    data: &[u8],
    arcade_card: bool,
) -> anyhow::Result<CurrentNativePceCdTasStateInspection> {
    inspect_current_native_pce_cd_tas_state_for_profile(machine, data, arcade_card, false)
}

pub fn inspect_current_native_pce_cd_tas_state_for_profile(
    machine: &PceMachine,
    data: &[u8],
    arcade_card: bool,
    memory_base: bool,
) -> anyhow::Result<CurrentNativePceCdTasStateInspection> {
    ensure_current_state(data)?;
    ensure_pce_cd_machine(machine, arcade_card, memory_base)?;
    let system_card_sha256: [u8; 32] = Sha256::digest(machine.hucard_rom()).into();
    let disc = machine
        .devices()
        .cdrom2()
        .expect("PC Engine CD TAS machine has CD hardware")
        .disc()
        .clone();
    let disc_sha256 = disc.content_hash();
    let mut candidate = PceMachine::with_cdrom2_system_card_controller_and_arcade_card(
        machine.hucard_rom().to_vec(),
        machine.hucard_board(),
        disc,
        PceConsoleWiring::PcEngine,
        ControllerPort::two_button(),
        arcade_card,
    )?;
    candidate
        .devices_mut()
        .controller_mut()
        .set_memory_base128_connected(memory_base);
    let psg = machine.devices().psg().debug_snapshot();
    candidate.set_sample_rate(psg.sample_rate);
    candidate.set_sample_generation_enabled(psg.sample_generation_enabled);
    decode_state(&mut candidate, data)?;
    ensure_pce_cd_machine(&candidate, arcade_card, memory_base)?;
    let presented = candidate.presented_frame();
    let controller_buttons = match candidate.devices().controller().device() {
        ControllerDevice::TwoButton(pad) => pad.buttons(),
        _ => unreachable!("PC Engine CD TAS state validation checked controller topology"),
    };
    Ok(CurrentNativePceCdTasStateInspection {
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
        system_card_sha256,
        system_card_len: candidate.hucard_rom().len(),
        disc_sha256,
        board: candidate.hucard_board(),
        wiring: candidate.devices().console_wiring(),
        psg_revision: candidate.devices().psg().revision(),
        controller_buttons,
        arcade_card_enabled: arcade_card,
        memory_base_enabled: memory_base,
    })
}

pub fn inspect_current_native_pce_cd_tas_state_identity(
    data: &[u8],
) -> anyhow::Result<CurrentNativePceCdTasStateIdentity> {
    inspect_current_native_pce_cd_tas_state_identity_for_arcade_card(data, false)
}

pub fn inspect_current_native_pce_cd_tas_state_identity_for_arcade_card(
    data: &[u8],
    arcade_card: bool,
) -> anyhow::Result<CurrentNativePceCdTasStateIdentity> {
    ensure_current_state(data)?;
    let mut reader = StateReader::new(data);
    let mut magic = [0; 8];
    reader.read_exact(&mut magic)?;
    let _version = reader.read_u32()?;
    ensure!(
        reader.read_u8()? == CONTENT_CD,
        "TAS state requires PC Engine CD media"
    );
    let mut system_card_sha256 = [0; 32];
    reader.read_exact(&mut system_card_sha256)?;
    let board = match reader.read_u8()? {
        tag if tag == board_to_tag(PceHuCardBoard::SystemCardV3) => PceHuCardBoard::SystemCardV3,
        _ => bail!("TAS state requires a Super System Card v3 board"),
    };
    ensure!(
        reader.read_u8()? == topology_to_tag(PceHardwareTopology::Base)
            && reader.read_u8()? == wiring_to_tag(PceConsoleWiring::PcEngine)
            && reader.read_u8()? == psg_revision_to_tag(PsgRevision::HuC6280)
            && reader.read_bool()? == arcade_card,
        "TAS state Arcade Card topology does not match the PC Engine CD TAS profile"
    );
    let mut disc_sha256 = [0; 32];
    reader.read_exact(&mut disc_sha256)?;
    Ok(CurrentNativePceCdTasStateIdentity {
        system_card_sha256,
        disc_sha256,
        board,
    })
}

pub fn validate_and_load_current_native_pce_cd_tas_state(
    machine: &mut PceMachine,
    data: &[u8],
) -> anyhow::Result<CurrentNativePceTasStateProjection> {
    validate_and_load_current_native_pce_cd_tas_state_for_arcade_card(machine, data, false)
}

pub fn validate_and_load_current_native_pce_cd_tas_state_for_arcade_card(
    machine: &mut PceMachine,
    data: &[u8],
    arcade_card: bool,
) -> anyhow::Result<CurrentNativePceTasStateProjection> {
    validate_and_load_current_native_pce_cd_tas_state_for_profile(machine, data, arcade_card, false)
}

pub fn validate_and_load_current_native_pce_cd_tas_state_for_profile(
    machine: &mut PceMachine,
    data: &[u8],
    arcade_card: bool,
    memory_base: bool,
) -> anyhow::Result<CurrentNativePceTasStateProjection> {
    let inspection = inspect_current_native_pce_cd_tas_state_for_profile(
        machine,
        data,
        arcade_card,
        memory_base,
    )?;
    decode_state(machine, data)?;
    Ok(inspection.projection)
}
