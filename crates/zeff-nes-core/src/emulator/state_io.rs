use crate::emulator::Emulator;
use zeff_emu_common::save_ram::SaveRamKind;

impl Emulator {
    pub fn save_ram_kind(&self) -> SaveRamKind {
        self.bus
            .cartridge
            .dump_battery_data()
            .filter(|data| !data.is_empty())
            .map_or(SaveRamKind::none(), |data| {
                SaveRamKind::known_battery_backed(data.len())
            })
    }

    pub fn has_battery(&self) -> bool {
        self.save_ram_kind().is_battery_backed()
    }

    pub fn dump_battery_sram(&self) -> Option<Vec<u8>> {
        self.bus
            .cartridge
            .dump_battery_data()
            .filter(|data| !data.is_empty())
    }

    pub fn load_battery_sram(&mut self, bytes: &[u8]) -> anyhow::Result<()> {
        self.invalidate_rollback_session();
        self.invalidate_audio_trace(
            zeff_emu_common::audio_trace::AudioTraceInvalidation::ExternalMutation,
        );
        self.bus.cartridge.load_battery_data(bytes)
    }

    pub fn dump_persistent_data(&self) -> Option<Vec<u8>> {
        self.bus.cartridge.dump_persistent_data()
    }

    pub fn load_persistent_data(&mut self, bytes: &[u8]) -> anyhow::Result<()> {
        self.invalidate_rollback_session();
        self.invalidate_audio_trace(
            zeff_emu_common::audio_trace::AudioTraceInvalidation::ExternalMutation,
        );
        self.bus.cartridge.load_persistent_data(bytes)
    }

    pub fn encode_state(&self) -> anyhow::Result<Vec<u8>> {
        crate::save_state::encode_state(self)
    }

    /// A fixed allocation bound for native saves of the loaded standard cartridge.
    pub fn max_encoded_state_bytes(&self) -> Option<usize> {
        if !self.has_standard_console_hardware()
            || !self.bus.cartridge.has_fixed_rollback_hardware()
        {
            return None;
        }
        // DMA bodies can add eight bytes; changing both controller types adds four.
        const OPTIONAL_RUNTIME_BYTES: usize = 12;
        let state = self.encode_state().ok()?;
        let raw_len = u32::from_le_bytes(state.get(12..16)?.try_into().ok()?) as usize;
        let maximum_raw_len = raw_len.checked_add(OPTIONAL_RUNTIME_BYTES)?;
        maximum_raw_len.checked_mul(2)?.checked_add(20)?;
        let maximum_compressed_len = lz4_flex::block::get_maximum_output_size(maximum_raw_len);
        maximum_compressed_len.checked_add(16)
    }

    pub fn load_state(&mut self, data: &[u8]) -> anyhow::Result<()> {
        let rollback_owner = self.rollback_owner.clone();
        let rollback_boundary = self.rollback_frame_boundary;
        let rollback_audio_trace = self.bus.audio_trace.clone();
        let rollback_state = self.encode_state()?;
        let rollback_mapper_runtime = self.bus.cartridge.capture_rollback_runtime_state();
        let rollback_cpu = self.cpu.clone();
        let rollback_ppu = self.bus.ppu.clone();
        let rollback_apu = self.bus.apu.clone();
        let rollback_bus = self.bus.capture_state_load_rollback();
        let rollback_debug = self.debug.clone();
        let rollback_opcode_log = self.opcode_log.clone();
        let rollback_instruction_trace = self.instruction_trace.clone();
        let rollback_call_stack = self.call_stack.clone();
        if let Err(error) = crate::save_state::decode_state(self, data) {
            crate::save_state::decode_state(self, &rollback_state)
                .expect("freshly encoded NES rollback state must decode");
            self.cpu = rollback_cpu;
            self.bus.ppu = rollback_ppu;
            self.bus.apu = rollback_apu;
            self.bus.restore_state_load_rollback(rollback_bus);
            self.bus
                .cartridge
                .restore_rollback_runtime_state(&rollback_mapper_runtime);
            self.debug = rollback_debug;
            self.opcode_log = rollback_opcode_log;
            self.instruction_trace = rollback_instruction_trace;
            self.call_stack = rollback_call_stack;
            self.bus.audio_trace = rollback_audio_trace;
            self.rollback_owner = rollback_owner;
            self.rollback_frame_boundary = rollback_boundary;
            return Err(error);
        }
        self.invalidate_rollback_session();
        self.rollback_frame_boundary = false;
        self.opcode_log.clear();
        self.instruction_trace.clear();
        self.call_stack.clear();
        Ok(())
    }

    pub fn load_state_from_bytes(&mut self, bytes: Vec<u8>) -> anyhow::Result<()> {
        self.load_state(&bytes)
    }
}
