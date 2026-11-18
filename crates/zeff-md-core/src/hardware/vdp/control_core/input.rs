#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::hardware::vdp) struct PhaseLevels {
    pub data_master: bool,
    pub data_slave: bool,
    pub horizontal_master: bool,
    pub horizontal_slave: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::hardware::vdp) struct MainRequests {
    pub write: bool,
    pub read: bool,
    pub control_select: bool,
    pub data_bus: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::hardware::vdp) struct FrontendFeedback {
    pub second_word_ready: bool,
    pub first_word_ready: bool,
    pub dma_bus_ready: bool,
    pub read_cycle_ready: bool,
    pub external_address_cycle: bool,
    pub cpu_data_capture: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::hardware::vdp) struct MemoryFeedback {
    pub read_selected: bool,
    pub write_selected: bool,
    pub fill_selected: bool,
    pub external_counter_cycle: bool,
    pub dma_active: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::hardware::vdp) struct PrimitiveInputs {
    pub phases: PhaseLevels,
    pub main: MainRequests,
    pub frontend: FrontendFeedback,
    pub memory: MemoryFeedback,
    pub reset_comb: bool,
    pub double_counter_test: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::hardware::vdp) struct HandshakeFeedback {
    pub request_gate: bool,
    pub recovery: bool,
    pub release_data_lanes: bool,
    pub data_produce_window: bool,
    pub write_control_latched: bool,
    pub read_control_latched: bool,
    pub request_sampled: bool,
    pub busy_first: bool,
    pub busy_second: bool,
    pub ready: bool,
    pub first_pending: bool,
    pub second_pending: bool,
    pub read_pending: bool,
    pub command_second: bool,
    pub command_second_n: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::hardware::vdp) struct Intake {
    pub first: bool,
    pub first_low: bool,
    pub second: bool,
    pub data_write: bool,
    pub status_read: bool,
    pub data_read: bool,
    pub control_ack: bool,
    pub second_dma_bit: bool,
    pub command_clear: bool,
    pub word_clear: bool,
    pub register_code_before: bool,
    pub recovery_before: bool,
    pub second_pending: bool,
    pub read_pending: bool,
    pub first_pending: bool,
}
