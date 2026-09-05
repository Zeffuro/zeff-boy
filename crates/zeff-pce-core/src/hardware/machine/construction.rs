use super::*;

impl PceMachine {
    pub fn new(hucard_rom: Vec<u8>) -> Result<Self, PceMachineError> {
        Self::with_cartridge_and_controller(
            hucard_rom,
            PceCartridgeDescriptor::default(),
            ControllerPort::default(),
        )
    }

    pub fn with_controller(
        hucard_rom: Vec<u8>,
        controller: ControllerPort,
    ) -> Result<Self, PceMachineError> {
        Self::with_cartridge_and_controller(
            hucard_rom,
            PceCartridgeDescriptor::default(),
            controller,
        )
    }

    pub fn with_psg_revision(
        hucard_rom: Vec<u8>,
        psg_revision: PsgRevision,
    ) -> Result<Self, PceMachineError> {
        Self::with_cartridge_controller_and_psg_revision(
            hucard_rom,
            PceCartridgeDescriptor::default(),
            ControllerPort::default(),
            psg_revision,
        )
    }

    pub fn with_cartridge(
        hucard_rom: Vec<u8>,
        cartridge: PceCartridgeDescriptor,
    ) -> Result<Self, PceMachineError> {
        Self::with_cartridge_and_controller(hucard_rom, cartridge, ControllerPort::default())
    }

    pub fn with_cartridge_and_controller(
        hucard_rom: Vec<u8>,
        cartridge: PceCartridgeDescriptor,
        controller: ControllerPort,
    ) -> Result<Self, PceMachineError> {
        let psg_revision = match cartridge.required_hardware() {
            PceCartridgeHardware::Base => PsgRevision::HuC6280,
            PceCartridgeHardware::SuperGrafx => PsgRevision::HuC6280A,
        };
        Self::with_cartridge_controller_and_psg_revision(
            hucard_rom,
            cartridge,
            controller,
            psg_revision,
        )
    }

    pub fn with_cdrom2(system_card_rom: Vec<u8>, disc: CdDisc) -> Result<Self, PceMachineError> {
        Self::with_cdrom2_and_controller(
            system_card_rom,
            disc,
            PceConsoleWiring::PcEngine,
            ControllerPort::default(),
        )
    }

    pub fn with_cdrom2_and_controller(
        system_card_rom: Vec<u8>,
        disc: CdDisc,
        console_wiring: PceConsoleWiring,
        controller: ControllerPort,
    ) -> Result<Self, PceMachineError> {
        Self::with_cdrom2_system_card_and_controller(
            system_card_rom,
            PceHuCardBoard::SystemCardV1V2,
            disc,
            console_wiring,
            controller,
        )
    }

    pub fn with_cdrom2_system_card_and_controller(
        system_card_rom: Vec<u8>,
        system_card_board: PceHuCardBoard,
        disc: CdDisc,
        console_wiring: PceConsoleWiring,
        controller: ControllerPort,
    ) -> Result<Self, PceMachineError> {
        Self::with_cdrom2_system_card_controller_and_arcade_card(
            system_card_rom,
            system_card_board,
            disc,
            console_wiring,
            controller,
            false,
        )
    }

    pub fn with_cdrom2_system_card_controller_and_arcade_card(
        system_card_rom: Vec<u8>,
        system_card_board: PceHuCardBoard,
        disc: CdDisc,
        console_wiring: PceConsoleWiring,
        controller: ControllerPort,
        arcade_card: bool,
    ) -> Result<Self, PceMachineError> {
        if !matches!(
            system_card_board,
            PceHuCardBoard::SystemCardV1V2 | PceHuCardBoard::SystemCardV3
        ) {
            return Err(PceMachineError::InvalidSystemCardBoard(system_card_board));
        }
        if arcade_card && system_card_board != PceHuCardBoard::SystemCardV3 {
            return Err(PceMachineError::InvalidSystemCardBoard(system_card_board));
        }
        let bus = BaseBus::with_hucard(
            system_card_rom,
            system_card_board,
            PceDevices::with_cdrom2_system_card_and_arcade_card(
                controller,
                console_wiring,
                disc,
                system_card_board == PceHuCardBoard::SystemCardV3,
                arcade_card,
            ),
        )
        .map_err(PceMachineError::BusConstruction)?;
        Ok(Self::finish_new(bus))
    }

    fn with_cartridge_controller_and_psg_revision(
        hucard_rom: Vec<u8>,
        cartridge: PceCartridgeDescriptor,
        controller: ControllerPort,
        psg_revision: PsgRevision,
    ) -> Result<Self, PceMachineError> {
        let topology = match cartridge.required_hardware() {
            PceCartridgeHardware::Base => PceHardwareTopology::Base,
            PceCartridgeHardware::SuperGrafx => PceHardwareTopology::SuperGrafx,
        };
        Self::with_topology(hucard_rom, cartridge, controller, psg_revision, topology)
    }

    pub(super) fn with_topology(
        hucard_rom: Vec<u8>,
        cartridge: PceCartridgeDescriptor,
        controller: ControllerPort,
        psg_revision: PsgRevision,
        topology: PceHardwareTopology,
    ) -> Result<Self, PceMachineError> {
        let board = cartridge.hucard_board(hucard_rom.len());
        let bus = BaseBus::with_hucard_and_topology(
            hucard_rom,
            board,
            topology,
            PceDevices::with_topology_console_wiring_and_psg_revision(
                topology,
                controller,
                cartridge.console_wiring(),
                psg_revision,
            ),
        )
        .map_err(PceMachineError::BusConstruction)?;
        Ok(Self::finish_new(bus))
    }

    fn finish_new(bus: BaseBus<PceDevices>) -> Self {
        let mut machine = Self {
            cpu: HuC6280::new(),
            bus,
            front_video: PceActiveOnlyVideoFrame::new(),
            back_video: PceActiveOnlyVideoFrame::new(),
            master_ticks: 0,
            vce_line_accumulator: 0,
            vdc_pixel_clock_remainder: 0,
            vce_line_index: 0,
            vce_frame_length: VceFrameLength::Lines262,
            faulted: false,
            execution_state: PceExecutionState::Running,
            suspend_after_instruction: false,
            skip_breakpoint_once: false,
            opcode_history: PceOpcodeHistory::default(),
            instruction_trace: InstructionTraceStore::default(),
            trace_scratch: TimedInstructionTrace::default(),
            trace_frame: 0,
            debug: AddressDebugController::new(),
            #[cfg(feature = "profiling")]
            profiling: PceProfiling::default(),
        };
        machine.reset();
        machine
    }

    #[cfg(test)]
    pub(in super::super) fn with_supergrafx_substrate_for_test(
        hucard_rom: Vec<u8>,
    ) -> Result<Self, PceMachineError> {
        Self::with_topology(
            hucard_rom,
            PceCartridgeDescriptor::default(),
            ControllerPort::default(),
            PsgRevision::HuC6280,
            PceHardwareTopology::SuperGrafx,
        )
    }

    pub fn reset(&mut self) {
        self.bus.reset_hucard();
        self.bus.devices_mut().reset();
        self.cpu.set_irq1_line(LineLevel::High);
        self.cpu.set_irq2_line(LineLevel::High);
        self.cpu.set_nmi_line(LineLevel::High);
        self.cpu.reset(&mut self.bus);
        self.front_video.begin_frame();
        self.back_video.begin_frame();
        self.master_ticks = 0;
        self.vce_line_accumulator = 0;
        self.vdc_pixel_clock_remainder = 0;
        self.vce_line_index = 0;
        self.vce_frame_length = self.bus.devices().vce().frame_length();
        self.faulted = false;
        self.execution_state = PceExecutionState::Running;
        self.suspend_after_instruction = false;
        self.skip_breakpoint_once = false;
        self.opcode_history.clear();
        self.instruction_trace.clear();
        self.trace_frame = 0;
        self.debug.clear_hits();
        #[cfg(feature = "profiling")]
        {
            self.profiling = PceProfiling::default();
        }
        self.refresh_cdrom2_irq2();
    }
}
