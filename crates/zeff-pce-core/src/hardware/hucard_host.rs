use anyhow::Context;
use sha2::{Digest, Sha256};
use zeff_emu_common::cheats::{
    CheatByteTarget, CheatPatch, apply_ram_cheats_16, apply_wide_ram_cheats,
};
use zeff_emu_common::save_ram::SaveRamKind;
use zeff_emu_common::save_state::{StateReader, StateWriter};

use super::{
    ControllerPort, PCE_HOST_FRAME_RGBA_BYTES, POPULOUS_HUCARD_RAM_LEN, PadButtons,
    PceCartridgeDescriptor, PceFrameRun, PceHuCardBoard, PceMachine, PceMachineError,
    project_full_raw_frame,
};

pub const HUCARD_BANK_LEN: usize = 0x2000;
pub const PCEAS_HEADER_LEN: usize = 0x200;
const STATE_MAGIC: &[u8; 8] = b"ZBPCEHC\0";
const STATE_VERSION: u32 = 1;
const MAX_CORE_STATE_BYTES: usize = 8 * 1024 * 1024;

pub struct PceHuCardHost {
    machine: PceMachine,
    framebuffer: Box<[u8]>,
    image_sha256: [u8; 32],
    frame_count: u64,
    pending_runtime_fault: Option<String>,
}

impl PceHuCardHost {
    /// Largest host-state payload accepted by `load_state`.
    pub const MAX_ENCODED_STATE_BYTES: usize = MAX_CORE_STATE_BYTES + 24;

    pub fn new(hucard_image: Vec<u8>, sample_rate: u32) -> anyhow::Result<Self> {
        let normalized = normalize_hucard_image(hucard_image)?;
        let image_sha256: [u8; 32] = Sha256::digest(&normalized).into();
        let cartridge = PceCartridgeDescriptor::from_sha256(image_sha256);
        let mut machine = PceMachine::with_cartridge_and_controller(
            normalized,
            cartridge,
            ControllerPort::two_button(),
        )?;
        machine.set_sample_rate(sample_rate);
        let mut host = Self {
            machine,
            framebuffer: vec![0; PCE_HOST_FRAME_RGBA_BYTES].into_boxed_slice(),
            image_sha256,
            frame_count: 0,
            pending_runtime_fault: None,
        };
        host.project_frame();
        Ok(host)
    }

    pub fn run_until_frame(&mut self) -> Result<PceFrameRun, PceMachineError> {
        let run = self.machine.run_until_frame()?;
        if run.frames_published() != 0 {
            self.frame_count = self.frame_count.saturating_add(run.frames_published());
            self.project_frame();
        }
        Ok(run)
    }

    pub fn step_frame(&mut self) {
        if self.machine.faulted() {
            return;
        }
        if let Err(error) = self.run_until_frame()
            && self.pending_runtime_fault.is_none()
        {
            self.pending_runtime_fault = Some(error.to_string());
        }
    }

    pub fn reset(&mut self) {
        self.machine.reset();
        self.frame_count = 0;
        self.pending_runtime_fault = None;
        self.project_frame();
    }

    pub fn set_input(&mut self, buttons: u8, dpad: u8) {
        let Some(pad) = self
            .machine
            .devices_mut()
            .controller_mut()
            .two_button_pad_mut()
        else {
            return;
        };
        pad.set_buttons(map_pad_buttons(buttons, dpad));
    }

    pub fn drain_audio_samples_into(&mut self, output: &mut Vec<f32>) {
        self.machine.drain_audio_samples_into(output);
    }

    pub fn encode_state(&self) -> anyhow::Result<Vec<u8>> {
        anyhow::ensure!(
            self.pending_runtime_fault.is_none(),
            "faulted PC Engine hosts cannot be saved"
        );
        let core_state = super::save_state::encode_state(&self.machine)
            .context("failed to encode PC Engine core state")?;
        let mut writer = StateWriter::with_capacity(core_state.len() + 24);
        writer.write_bytes(STATE_MAGIC);
        writer.write_u32(STATE_VERSION);
        writer.write_u64(self.frame_count);
        writer.write_vec(&core_state);
        Ok(writer.into_bytes())
    }

    pub fn load_state(&mut self, data: &[u8]) -> anyhow::Result<()> {
        let mut reader = StateReader::new(data);
        let mut magic = [0; 8];
        reader.read_exact(&mut magic)?;
        anyhow::ensure!(
            &magic == STATE_MAGIC,
            "not a valid PC Engine HuCard host state"
        );
        let version = reader.read_u32()?;
        anyhow::ensure!(
            version == STATE_VERSION,
            "unsupported PC Engine HuCard host state version {version}"
        );
        let frame_count = reader.read_u64()?;
        let core_state = reader.read_vec(MAX_CORE_STATE_BYTES)?;
        anyhow::ensure!(
            reader.is_exhausted(),
            "PC Engine HuCard host state has unexpected trailing data"
        );

        super::save_state::decode_state(&mut self.machine, &core_state)
            .context("failed to decode PC Engine core state")?;
        self.frame_count = frame_count;
        self.pending_runtime_fault = None;
        self.project_frame();
        Ok(())
    }

    pub fn framebuffer(&self) -> &[u8] {
        &self.framebuffer
    }

    pub fn machine(&self) -> &PceMachine {
        &self.machine
    }

    pub fn machine_mut(&mut self) -> &mut PceMachine {
        &mut self.machine
    }

    pub const fn image_sha256(&self) -> [u8; 32] {
        self.image_sha256
    }

    pub const fn frame_count(&self) -> u64 {
        self.frame_count
    }

    pub const fn max_encoded_state_bytes(&self) -> usize {
        Self::MAX_ENCODED_STATE_BYTES
    }

    pub fn take_runtime_fault(&mut self) -> Option<String> {
        self.pending_runtime_fault.take()
    }

    pub fn apply_cheats(&mut self, patches: &[CheatPatch]) {
        apply_pce_cheats(&mut self.machine, patches);
    }

    pub fn save_ram_kind(&self) -> SaveRamKind {
        match self.machine.hucard_board() {
            PceHuCardBoard::Populous => SaveRamKind::mapper_ram_unknown(POPULOUS_HUCARD_RAM_LEN),
            PceHuCardBoard::Plain
            | PceHuCardBoard::Sf2Ce
            | PceHuCardBoard::SystemCardV1V2
            | PceHuCardBoard::SystemCardV3 => SaveRamKind::none(),
        }
    }

    fn project_frame(&mut self) {
        project_full_raw_frame(
            self.machine.presented_frame(),
            self.machine.hardware_topology(),
            &mut self.framebuffer,
        );
    }
}

pub fn apply_pce_cheats(machine: &mut PceMachine, patches: &[CheatPatch]) {
    apply_ram_cheats_16(machine, patches);
    let mut physical_ram = PcePhysicalRam { machine };
    apply_wide_ram_cheats(&mut physical_ram, patches);
}

pub fn normalize_hucard_image(hucard_image: Vec<u8>) -> anyhow::Result<Vec<u8>> {
    if hucard_image.len().is_multiple_of(HUCARD_BANK_LEN) {
        anyhow::ensure!(!hucard_image.is_empty(), "PC Engine HuCard image is empty");
        return Ok(hucard_image);
    }
    let payload_len = hucard_image.len().saturating_sub(PCEAS_HEADER_LEN);
    let has_pceas_header = hucard_image.len() > PCEAS_HEADER_LEN
        && payload_len.is_multiple_of(HUCARD_BANK_LEN)
        && usize::from(hucard_image[0]) == payload_len / HUCARD_BANK_LEN
        && hucard_image[1..PCEAS_HEADER_LEN]
            .iter()
            .all(|&byte| byte == 0);
    anyhow::ensure!(
        has_pceas_header,
        "PC Engine HuCard image length must be a multiple of {HUCARD_BANK_LEN} bytes or carry a valid PCEAS header"
    );
    Ok(hucard_image[PCEAS_HEADER_LEN..].to_vec())
}

fn map_pad_buttons(buttons: u8, dpad: u8) -> PadButtons {
    let mut mapped = PadButtons::empty();
    mapped.set(PadButtons::I, buttons & 0x01 != 0);
    mapped.set(PadButtons::II, buttons & 0x02 != 0);
    mapped.set(PadButtons::SELECT, buttons & 0x04 != 0);
    mapped.set(PadButtons::RUN, buttons & 0x08 != 0);
    mapped.set(PadButtons::RIGHT, dpad & 0x01 != 0);
    mapped.set(PadButtons::LEFT, dpad & 0x02 != 0);
    mapped.set(PadButtons::UP, dpad & 0x04 != 0);
    mapped.set(PadButtons::DOWN, dpad & 0x08 != 0);
    mapped
}

struct PcePhysicalRam<'a> {
    machine: &'a mut PceMachine,
}

impl CheatByteTarget<u32> for PcePhysicalRam<'_> {
    fn cheat_peek8(&self, address: u32) -> u8 {
        self.machine.cheat_peek_physical_ram(address).unwrap_or(0)
    }

    fn cheat_write8(&mut self, address: u32, value: u8) {
        self.machine.cheat_write_physical_ram(address, value);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_rom() -> Vec<u8> {
        let mut rom = vec![0xEA; HUCARD_BANK_LEN];
        rom[0x1FFE] = 0x00;
        rom[0x1FFF] = 0xE0;
        rom
    }

    #[test]
    fn pceas_header_is_removed_before_hashing() {
        let rom = test_rom();
        let plain = PceHuCardHost::new(rom.clone(), 48_000).unwrap();
        let mut headered = vec![0; PCEAS_HEADER_LEN];
        headered[0] = 1;
        headered.extend_from_slice(&rom);
        let headered = PceHuCardHost::new(headered, 48_000).unwrap();

        assert_eq!(headered.image_sha256(), plain.image_sha256());
        assert_eq!(
            headered.machine().hucard_rom(),
            plain.machine().hucard_rom()
        );
    }

    #[test]
    fn state_is_larger_than_four_mib_and_reprojects_after_restore() {
        let mut host = PceHuCardHost::new(test_rom(), 48_000).unwrap();
        host.set_input(0x03, 0x05);
        host.step_frame();
        let state = host.encode_state().unwrap();
        assert!(state.len() > 4 * 1024 * 1024);
        let expected_frame = host.framebuffer().to_vec();

        host.reset();
        host.load_state(&state).unwrap();

        assert_eq!(host.encode_state().unwrap(), state);
        assert_eq!(host.framebuffer(), expected_frame);
    }

    #[test]
    fn logical_and_physical_cheats_share_native_work_ram_semantics() {
        use zeff_emu_common::cheats::CheatValue;

        let mut host = PceHuCardHost::new(test_rom(), 48_000).unwrap();
        host.machine_mut()
            .cpu_mut()
            .cpu_mut()
            .set_mapping_register(2, 0xF8);
        host.apply_cheats(&[
            CheatPatch::RamWrite {
                address: 0x4005,
                value: CheatValue::Constant(0x42),
            },
            CheatPatch::WideRamWrite {
                address: 0x1F_2345,
                value: CheatValue::Constant(0x66),
            },
        ]);

        assert_eq!(host.machine().mapped_work_ram()[5], 0x42);
        assert_eq!(host.machine().mapped_work_ram()[0x345], 0x66);
    }

    #[test]
    fn wide_physical_cheats_cover_system_card_and_cd_work_ram() {
        use super::super::{
            CDROM2_WORK_RAM_START, CdDisc, CdTrack, CdTrackMode, PceConsoleWiring,
            SUPER_SYSTEM_CARD_RAM_START, SYSTEM_CARD_V1_V2_IMAGE_LEN,
        };
        use zeff_emu_common::cheats::CheatValue;

        let track =
            CdTrack::from_index1_data(1, 4, None, 0, CdTrackMode::Mode1_2048, vec![0; 2048])
                .unwrap();
        let disc = CdDisc::new(vec![track]).unwrap();
        let mut machine = PceMachine::with_cdrom2_system_card_and_controller(
            vec![0; SYSTEM_CARD_V1_V2_IMAGE_LEN],
            PceHuCardBoard::SystemCardV3,
            disc,
            PceConsoleWiring::PcEngine,
            ControllerPort::two_button(),
        )
        .unwrap();
        let system_card_address = SUPER_SYSTEM_CARD_RAM_START + 0x1234;
        let cd_address = CDROM2_WORK_RAM_START + 0x2345;

        apply_pce_cheats(
            &mut machine,
            &[
                CheatPatch::WideRamWrite {
                    address: system_card_address,
                    value: CheatValue::Constant(0x5A),
                },
                CheatPatch::WideRamWrite {
                    address: cd_address,
                    value: CheatValue::Constant(0x6B),
                },
                CheatPatch::WideRamWriteIfEquals {
                    address: cd_address,
                    value: CheatValue::Constant(0x7C),
                    compare: CheatValue::Constant(0x6B),
                },
            ],
        );

        assert_eq!(machine.debug_peek_physical8(system_card_address), 0x5A);
        assert_eq!(machine.debug_peek_physical8(cd_address), 0x7C);
    }
}
