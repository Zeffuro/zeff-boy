mod arcade_card;
mod blip_buf;
mod bus;
mod cartridge;
mod cartridge_catalog;
mod cd_media;
mod cdrom2;
mod constants;
mod controller;
pub mod cpu;
mod host_video;
mod hucard_host;
mod machine;
mod pce_devices;
#[cfg(feature = "profiling")]
mod profiling;
mod psg;
pub mod save_state;
mod vce;
mod vdc;
mod vdc_composite;
mod vdc_horizontal;
mod vdc_render;
mod vdc_scanline;
mod vdc_sprite_render;
mod vdc_video;
mod vpc;

pub use arcade_card::{
    ARCADE_CARD_RAM_LEN, ArcadeCard, ArcadeCardDebugSnapshot, ArcadeCardPortDebugSnapshot,
    PceArcadeCardMode,
};
pub use bus::{
    BaseBus, BaseBusDevices, BaseBusError, BaseBusErrorKind, HUCARD_ROM_REGION_LEN, OPEN_BUS_VALUE,
    PceHardwareTopology, PhysicalRegion, PsgPort, SUPERGRAFX_WORK_RAM_LEN, VcePort, WORK_RAM_LEN,
    decode_physical_region, decode_physical_region_for,
};
pub use cartridge::{
    POPULOUS_CANONICAL_SHA256, POPULOUS_HUCARD_IMAGE_LEN, POPULOUS_HUCARD_RAM_LEN,
    PceCartridgeDescriptor, PceCartridgeHardware, PceConsoleWiring, PceHuCardBoard,
    SF2_CE_CANONICAL_SHA256, SF2_CE_HUCARD_IMAGE_LEN, SUPER_SYSTEM_CARD_RAM_END,
    SUPER_SYSTEM_CARD_RAM_LEN, SUPER_SYSTEM_CARD_RAM_START, SYSTEM_CARD_V1_V2_IMAGE_LEN,
};
pub use cd_media::{
    CD_RAW_SECTOR_BYTES, CD_USER_SECTOR_BYTES, CdDisc, CdDiscError, CdReadError, CdSourceError,
    CdTrack, CdTrackMode, CdTrackSource,
};
pub use cdrom2::{
    CDROM2_ADPCM_RAM_LEN, CDROM2_BRAM_END, CDROM2_BRAM_LEN, CDROM2_BRAM_START, CDROM2_REGISTER_END,
    CDROM2_REGISTER_START, CDROM2_WORK_RAM_END, CDROM2_WORK_RAM_LEN, CDROM2_WORK_RAM_START,
    CdAdpcmDebugSnapshot, CdAudioDebugSnapshot, CdAudioEndMode, CdAudioFadeTarget, CdAudioStatus,
    CdProtocolEventDebugKind, CdProtocolEventDebugSnapshot, CdRom2, CdRom2DebugSnapshot,
    CdScsiPhase, PROVISIONAL_CDROM2_ADPCM_MIX_GAIN,
    PROVISIONAL_CDROM2_ADPCM_RATE_WRITE_PRESERVES_PHASE,
    PROVISIONAL_CDROM2_ADPCM_RESTART_REQUIRES_END_CLEAR_OR_D6_CLEAR,
    PROVISIONAL_CDROM2_ADPCM_STOP_AT_NEXT_NIBBLE_BOUNDARY, PROVISIONAL_CDROM2_AUTO_ACK_TICKS,
    PROVISIONAL_CDROM2_FADE_LONG_STEP_TICKS, PROVISIONAL_CDROM2_FADE_SHORT_STEP_TICKS,
    PROVISIONAL_CDROM2_NEXT_REQUEST_TICKS, PROVISIONAL_CDROM2_PHASE_TICKS,
    PROVISIONAL_CDROM2_READ_STARTUP_SECTORS, PROVISIONAL_CDROM2_SELECTION_TICKS,
};
pub use constants::{
    PCE_DEFAULT_AUDIO_SAMPLE_RATE_HZ, PCE_MASTER_CLOCK_NTSC_REFERENCE_MULTIPLIER,
    PCE_NTSC_COLORBURST_CLOCK_HZ_DENOMINATOR, PCE_NTSC_COLORBURST_CLOCK_HZ_NUMERATOR,
    PCE_NTSC_MASTER_CLOCK_COLORBURST_MULTIPLIER, PCE_NTSC_MASTER_CLOCK_HZ_DENOMINATOR,
    PCE_NTSC_MASTER_CLOCK_HZ_NUMERATOR, PCE_NTSC_REFERENCE_MHZ_DENOMINATOR,
    PCE_NTSC_REFERENCE_MHZ_NUMERATOR,
};
pub use controller::{
    ControllerDevice, ControllerDeviceDebugSnapshot, ControllerPort, ControllerPortDebugSnapshot,
    DETERMINISTIC_SIX_BUTTON_RESET_PHASE, FivePortMultitap, MAX_CONTROLLER_STATE_SECTION_BYTES,
    MEMORY_BASE128_RAM_LEN, MULTITAP_EXHAUSTED_NIBBLE, MemoryBase128, MemoryBase128DebugSnapshot,
    MemoryBase128Phase, MouseScanPhase, MultitapDevice, MultitapDeviceDebugSnapshot, MultitapPort,
    PROVISIONAL_MOUSE_CLEAR_TIMEOUT_MASTER_TICKS, PROVISIONAL_MOUSE_SELECT_TIMEOUT_MASTER_TICKS,
    PadButtons, PceControllerMode, PceMemoryBaseMode, PceMouse, PceMouseDebugSnapshot,
    SixButtonExtraButtons, SixButtonPad, SixButtonPhase, TwoButtonPad,
};
pub use cpu::{IrqPort, TimerPort, physical_address_for_page};
pub use host_video::{
    PCE_HOST_FRAME_HEIGHT, PCE_HOST_FRAME_RGBA_BYTES, PCE_HOST_FRAME_WIDTH, project_full_raw_frame,
};
pub use hucard_host::{
    HUCARD_BANK_LEN, PCEAS_HEADER_LEN, PceHuCardHost, apply_pce_cheats, normalize_hucard_image,
};
pub use machine::{
    PCE_VDC_VCE_ACCESS_WAIT_CYCLES, PROVISIONAL_PCE_CPU_ACTION_USES_ENTERING_SPEED,
    PROVISIONAL_PCE_HIGH_SPEED_MASTER_TICKS_PER_CPU_CYCLE,
    PROVISIONAL_PCE_LOW_SPEED_MASTER_TICKS_PER_CPU_CYCLE,
    PROVISIONAL_PCE_MASTER_TICKS_PER_VCE_LINE,
    PROVISIONAL_PCE_NON_PSG_DEVICE_ADVANCEMENT_IS_INSTRUCTION_ATOMIC,
    PROVISIONAL_PCE_VSYNC_ASSERT_NORMALIZED_TO_LINE_ZERO, PceClockCounter, PceCpuAction,
    PceCpuDebugSnapshot, PceExecutionState, PceFrameRun, PceMachine, PceMachineError,
    PceMachineStep, PceOpcodeHistoryEntry,
};
pub use pce_devices::{
    BASE_PCE_CDROM2_CONTROLLER_UPPER_BITS, BASE_PCE_NO_CD_CONTROLLER_UPPER_BITS,
    BASE_TURBOGRAFX16_CDROM2_CONTROLLER_UPPER_BITS, BASE_TURBOGRAFX16_NO_CD_CONTROLLER_UPPER_BITS,
    PceDevices, PceHardwareDebugSnapshot, SuperGrafxVideo,
};
#[cfg(feature = "profiling")]
pub use profiling::PceProfilingSnapshot;
pub use psg::{
    DEFAULT_PSG_SAMPLE_RATE, DETERMINISTIC_PSG_RESET_ATTENUATION_SLOT,
    DETERMINISTIC_PSG_RESET_CLEARS_WAVE_RAM, DETERMINISTIC_PSG_RESET_VALUE, HuC6280Psg,
    MAX_PSG_SAMPLE_RATE, PROVISIONAL_HUC6280_KEYED_WAVE_WRITE_MATCHES_HUC6280A,
    PROVISIONAL_PSG_GAIN_LATCH_DELAY_CLOCKS, PROVISIONAL_PSG_GAIN_SCAN_CLOCKS_PER_PASS,
    PROVISIONAL_PSG_NOISE_ZERO_PERIOD, PSG_CHANNEL_COUNT, PSG_CLOCK_DENOMINATOR,
    PSG_CLOCK_NUMERATOR, PSG_DEBUG_WAVEFORM_RATE_HZ, PSG_DEBUG_WAVEFORM_SAMPLE_COUNT,
    PSG_INTERNAL_CLOCK_NUMERATOR, PSG_INTERNAL_MASTER_CLOCK_DIVISOR, PSG_MASTER_CLOCK_DIVISOR,
    PSG_UNAVAILABLE_READ_VALUE, PSG_WAVEFORM_WORDS, PSG_ZERO_FREQUENCY_PERIOD, PsgChannel,
    PsgChannelDebugSnapshot, PsgDebugSnapshot, PsgRevision,
};
pub use vce::{
    DETERMINISTIC_VCE_INITIAL_COLOR, DETERMINISTIC_VCE_RESET_PRESERVES_PALETTE,
    DETERMINISTIC_VCE_RESET_VALUE, HuC6260, VCE_PALETTE_COLORS, VCE_UNAVAILABLE_READ_VALUE,
    VceColor, VceDebugSnapshot, VcePixelClock,
};
pub use vdc::{
    DETERMINISTIC_VDC_INITIAL_SATB_WORD, DETERMINISTIC_VDC_INITIAL_VRAM_WORD,
    DETERMINISTIC_VDC_RESET_CLEARS_SATB, DETERMINISTIC_VDC_RESET_PRESERVES_VRAM,
    DETERMINISTIC_VDC_RESET_VALUE, HuC6270, VDC_SATB_WORDS, VDC_UNAVAILABLE_READ_VALUE,
    VDC_VRAM_BYTES, VDC_VRAM_WORD_ADDRESS_MASK, VDC_VRAM_WORDS, VdcDebugSnapshot, VdcDmaAccess,
    VdcDmaChannel, VdcDmaDirection, VdcDmaError, VdcDmaProgress, VdcRegister, VdcStatus,
    VramDmaState, VramSatbDmaState,
};
pub use vdc_composite::{
    CompositedPixel, DisplayCompositionError, DisplayLayerLine, compose_vdc_output_scanline,
};
pub use vdc_horizontal::{
    DETERMINISTIC_VDC_RESET_FRAME_BURST, PROVISIONAL_VCE_CLOCK_DIVIDER_PRESERVES_MASTER_PHASE,
    PROVISIONAL_VDC_DMA_SATB_FIRST, PROVISIONAL_VDC_REJECTS_ACTIVE_NONBURST_DMA_TRIGGER,
    PROVISIONAL_VDC_REJECTS_DMA_TRIGGER_WHILE_ACTIVE, VDC_DMA_PIXELS_PER_WORD,
    VdcHorizontalAdvance, VdcHorizontalPhase, VdcPortWriteResult, VdcVramDmaTriggerResult,
};
pub use vdc_render::{
    BackgroundColorMode, BackgroundRenderError, BackgroundRenderState, BackgroundScanlineStatus,
};
pub use vdc_scanline::{
    DETERMINISTIC_VDC_RESET_LATCHED_MEMORY_WIDTH,
    PROVISIONAL_EXTERNAL_VCE_VERTICAL_PROFILE_LATCHED_AT_VSYNC,
    PROVISIONAL_EXTERNAL_VDW_CAPPED_TO_VCE_FRAME,
    PROVISIONAL_EXTERNAL_VSYNC_MARKER_RESTARTS_VERTICAL_PROGRESSION,
    PROVISIONAL_STOCK_MACHINE_VCE_BOUNDARIES_DRIVE_VDC_HORIZONTAL_AND_VERTICAL_SYNC,
    VceFrameLength, VdcActiveDisplayLine, VdcExternalVceScanline, VdcScanlineAdvanceError,
    VdcScanlineBoundary, VdcScanlineTransition, VdcSyncOutput, VdcVerticalPhase,
};
pub use vdc_sprite_render::{
    SpriteBackgroundPriority, SpriteColorMode, SpritePixel, SpriteRenderError, SpriteRenderState,
    SpriteScanlineEvents, SpriteScanlineStatus,
};
pub use vdc_video::{
    PCE_ACTIVE_FRAME_HEIGHT, PCE_ACTIVE_FRAME_RGBA_BYTES, PCE_ACTIVE_FRAME_UNUSED_RGBA,
    PCE_ACTIVE_FRAME_WIDTH, PCE_SIGNAL_FIRST_ROW, PCE_SIGNAL_ROW_END, PceActiveOnlyVideoFrame,
    PcePresentedFrame, PceVideoActiveBounds, PceVideoRenderError, PceVideoRowMetadata,
    PceVideoSignalBounds,
};
pub use vpc::{
    DETERMINISTIC_VPC_RESET_REGISTERS, HuC6202, PROVISIONAL_VPC_PRIORITY_MODE_POLICY,
    PROVISIONAL_VPC_WINDOW_ORIGIN_AND_THRESHOLD, VpcDebugSnapshot, VpcPixelSelection,
    VpcPixelSource, VpcPort, VpcPriorityModePolicy, VpcVdc, VpcVdcPixel, VpcWindow,
    VpcWindowRegion,
};

#[cfg(test)]
mod arcade_card_tests;
#[cfg(test)]
mod bus_tests;
#[cfg(test)]
mod cartridge_tests;
#[cfg(test)]
mod cd_media_tests;
#[cfg(test)]
mod cdrom2_tests;
#[cfg(test)]
mod controller_tests;
#[cfg(test)]
mod machine_tests;
#[cfg(test)]
mod psg_tests;
#[cfg(test)]
mod supergrafx_tests;
#[cfg(test)]
mod vce_tests;
#[cfg(test)]
mod vdc_composite_tests;
#[cfg(test)]
mod vdc_dma_tests;
#[cfg(test)]
mod vdc_horizontal_tests;
#[cfg(test)]
mod vdc_render_tests;
#[cfg(test)]
mod vdc_scanline_tests;
#[cfg(test)]
mod vdc_sprite_render_tests;
#[cfg(test)]
mod vdc_tests;
#[cfg(test)]
mod vdc_video_tests;
#[cfg(test)]
mod vpc_tests;
