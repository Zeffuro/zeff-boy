use super::{InputDevicesPage, SettingsCategory};

pub(super) struct SearchEntry {
    pub(super) id: SettingId,
    pub(super) title: &'static str,
    pub(super) category: SettingsCategory,
    pub(super) input_page: InputDevicesPage,
    pub(super) keywords: &'static str,
}

macro_rules! setting_registry {
    ($( $(#[$attr:meta])* $id:ident, $category:ident, $page:ident, $title:literal, $keywords:literal; )*) => {
        #[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
        pub(super) enum SettingId {
            $( $(#[$attr])* $id, )*
        }

        const ENTRIES: &[SearchEntry] = &[
            $( $(#[$attr])* SearchEntry {
                id: SettingId::$id,
                title: $title,
                category: SettingsCategory::$category,
                input_page: InputDevicesPage::$page,
                keywords: $keywords,
            }, )*
        ];
    };
}

setting_registry! {
    InputDevicesMappingScope, InputDevices, Controls, "Mapping scope", "global system game per-game overrides inherit settings";
    InputDevicesRenameSelectedProfile, InputDevices, Controls, "Rename selected profile", "controller saved name manage";
    InputDevicesLoadProfileShortcuts, InputDevices, Controls, "Load saved emulator shortcuts", "controller profile legacy global hotkeys";
    InputDevicesDPadUpMapping, InputDevices, Controls, "D-pad Up mapping", "up direction keyboard key controller gamepad binding";
    InputDevicesDPadDownMapping, InputDevices, Controls, "D-pad Down mapping", "down direction keyboard key controller gamepad binding";
    InputDevicesDPadLeftMapping, InputDevices, Controls, "D-pad Left mapping", "left direction keyboard key controller gamepad binding";
    InputDevicesDPadRightMapping, InputDevices, Controls, "D-pad Right mapping", "right direction keyboard key controller gamepad binding";
    InputDevicesAButtonMapping, InputDevices, Controls, "A button mapping", "keyboard key controller gamepad binding";
    InputDevicesBButtonMapping, InputDevices, Controls, "B button mapping", "keyboard key controller gamepad binding";
    InputDevicesXButtonMapping, InputDevices, Controls, "X button mapping", "snes pce keyboard key controller gamepad binding";
    InputDevicesYButtonMapping, InputDevices, Controls, "Y button mapping", "snes pce keyboard key controller gamepad binding";
    InputDevicesLShoulderMapping, InputDevices, Controls, "L shoulder mapping", "gba snes keyboard key controller gamepad binding";
    InputDevicesRShoulderMapping, InputDevices, Controls, "R shoulder mapping", "gba snes keyboard key controller gamepad binding";
    InputDevicesStartButtonMapping, InputDevices, Controls, "Start button mapping", "keyboard key controller gamepad binding";
    InputDevicesSelectButtonMapping, InputDevices, Controls, "Select button mapping", "keyboard key controller gamepad binding";
    InputDevicesWonderswanX1Mapping, InputDevices, Controls, "WonderSwan X1 mapping", "ws direct keyboard key controller gamepad binding";
    InputDevicesWonderswanX2Mapping, InputDevices, Controls, "WonderSwan X2 mapping", "ws direct keyboard key controller gamepad binding";
    InputDevicesWonderswanX3Mapping, InputDevices, Controls, "WonderSwan X3 mapping", "ws direct keyboard key controller gamepad binding";
    InputDevicesWonderswanX4Mapping, InputDevices, Controls, "WonderSwan X4 mapping", "ws direct keyboard key controller gamepad binding";
    InputDevicesWonderswanY1Mapping, InputDevices, Controls, "WonderSwan Y1 mapping", "ws direct keyboard key controller gamepad binding";
    InputDevicesWonderswanY2Mapping, InputDevices, Controls, "WonderSwan Y2 mapping", "ws direct keyboard key controller gamepad binding";
    InputDevicesWonderswanY3Mapping, InputDevices, Controls, "WonderSwan Y3 mapping", "ws direct keyboard key controller gamepad binding";
    InputDevicesWonderswanY4Mapping, InputDevices, Controls, "WonderSwan Y4 mapping", "ws direct keyboard key controller gamepad binding";
    InputDevicesWonderswanAMapping, InputDevices, Controls, "WonderSwan A mapping", "ws direct keyboard key controller gamepad binding";
    InputDevicesWonderswanBMapping, InputDevices, Controls, "WonderSwan B mapping", "ws direct keyboard key controller gamepad binding";
    InputDevicesWonderswanStartMapping, InputDevices, Controls, "WonderSwan Start mapping", "ws direct keyboard key controller gamepad binding";
    InputDevicesTiltUpKey, InputDevices, TestAndCalibrate, "Tilt Up key", "mbc7 keyboard tilt binding";
    InputDevicesTiltDownKey, InputDevices, TestAndCalibrate, "Tilt Down key", "mbc7 keyboard tilt binding";
    InputDevicesTiltLeftKey, InputDevices, TestAndCalibrate, "Tilt Left key", "mbc7 keyboard tilt binding";
    InputDevicesTiltRightKey, InputDevices, TestAndCalibrate, "Tilt Right key", "mbc7 keyboard tilt binding";
    GeneralPauseWhenWindowLosesFocus, General, Controls, "Pause when window loses focus", "background inactive focus pause";
    #[cfg(not(target_arch = "wasm32"))]
    GeneralCheckForUpdatesOnStartup, General, Controls, "Check for updates on startup", "version upgrade launch startup";
    GeneralClearRecentContent, General, Controls, "Clear recent content", "history recent rom games files list";
    GeneralResetAllPreferences, General, Controls, "Reset all preferences", "defaults restore settings";
    StorageRetrySavingSettings, Storage, Controls, "Retry saving settings", "persistence error recovery save preferences";
    StorageExportSettings, Storage, Controls, "Export settings", "preferences json backup portable save file";
    StorageImportSettings, Storage, Controls, "Import settings", "preferences json restore file";
    #[cfg(target_arch = "wasm32")]
    StorageLoadSavedSettings, Storage, Controls, "Load saved settings after conflict", "browser tab writer concurrent conflict recover reload";
    AudioMasterVolume, Audio, Controls, "Master volume", "sound loudness mute";
    AudioMuteAudioWhileFastForwardIsHeld, Audio, Controls, "Mute audio while fast-forward is held", "sound speedup fast forward";
    AudioEmulatorSampleRate, Audio, Controls, "Emulator sample rate", "output sound hz resampling 32000 44100 48000 96000 192000";
    #[cfg(not(target_arch = "wasm32"))]
    AudioOutputDevice, Audio, Controls, "Audio output device", "sound speakers headphones system default host device";
    #[cfg(not(target_arch = "wasm32"))]
    AudioBufferPolicy, Audio, Controls, "Audio buffer policy", "sound queue latency underrun balanced stable";
    #[cfg(not(target_arch = "wasm32"))]
    AudioRetryOutput, Audio, Controls, "Retry audio output", "sound device reconnect recovery retry";
    AudioEnableLowPassOutputFilter, Audio, Controls, "Enable low-pass output filter", "sound low pass filtering";
    AudioLowPassCutoff, Audio, Controls, "Low-pass cutoff", "filter frequency hz";
    AudioRecordingFormat, Audio, Controls, "Recording format", "sound wav ogg vorbis pcm";
    VideoVsync, Video, Controls, "VSync", "vertical synchronization tearing adaptive";
    VideoScalingMode, Video, Controls, "Scaling mode", "nearest linear hq2x xbr2x eagle2x upscale pixel";
    VideoOffscreenScale, Video, Controls, "Offscreen scale", "resolution render target size";
    VideoEdgeStrength, Video, Controls, "Edge strength", "shader scaling upscaler hq2x xbr2x eagle2x";
    VideoEffect, Video, Controls, "Effect", "shader scanlines lcd grid crt gbc palette custom";
    VideoScanlineIntensity, Video, Controls, "Scanline intensity", "shader effect scanlines crt";
    VideoLcdGridIntensity, Video, Controls, "LCD grid intensity", "shader effect lcd grid";
    VideoCrtCurvature, Video, Controls, "CRT curvature", "shader effect crt curved";
    VideoPaletteMix, Video, Controls, "Palette mix", "shader effect gbc palette";
    VideoPaletteWarmth, Video, Controls, "Palette warmth", "shader effect gbc palette";
    VideoLoadCustomWgslShader, Video, Controls, "Load custom WGSL shader", "effect fragment file path import";
    VideoClearCustomShader, Video, Controls, "Clear custom shader", "effect wgsl fragment path reset";
    VideoGbGbcColorCorrection, Video, Controls, "GB/GBC color correction", "game boy color custom matrix lcd";
    VideoGbGbcCustomColorMatrix, Video, Controls, "GB/GBC custom color matrix", "game boy color correction rgb identity gbc matrix";
    VideoDmgPalette, Video, Controls, "DMG palette", "game boy monochrome green grayscale";
    VideoGbaColorCorrection, Video, Controls, "GBA color correction", "game boy advance lcd custom";
    VideoGbaCustomColorMatrix, Video, Controls, "GBA custom color matrix", "game boy advance rgb correction identity";
    VideoWsColorCorrection, Video, Controls, "WS color correction", "wonderswan swancrystal lcd custom";
    VideoWonderswanCustomColorMatrix, Video, Controls, "WonderSwan custom color matrix", "ws wsc lcd rgb correction identity";
    VideoNesPaletteMode, Video, Controls, "NES palette mode", "nintendo colors custom pal";
    VideoLoadNesPaletteFile, Video, Controls, "Load NES palette file", "custom nes .pal import";
    VideoClearNesPaletteFile, Video, Controls, "Clear NES palette file", "custom nes .pal reset";
    VideoPcEngineVisibleArea, Video, Controls, "PC Engine visible area", "pce overscan crop display";
    VideoPcEngineColorOutput, Video, Controls, "PC Engine color output", "pce palette rgb composite";
    EmulationFastForwardMultiplier, Emulation, Controls, "Fast-forward multiplier", "speed speedup fast forward";
    EmulationStartInSlowMotionMode, Emulation, Controls, "Start in slow-motion mode", "speed slow motion";
    EmulationSlowMotionDivisor, Emulation, Controls, "Slow-motion divisor", "speed slow motion";
    EmulationUncappedFramesTick, Emulation, Controls, "Uncapped frames/tick", "speed throughput latency batch";
    EmulationStartInUncappedMode, Emulation, Controls, "Start in uncapped mode", "speed unlimited";
    EmulationFrameSkipWhenBehind, Emulation, Controls, "Frame skip when behind", "timing speed frameskip";
    Emulation7zDecoderMemoryLimit, Emulation, Controls, "7z decoder memory limit", "archive pc engine pce cd ram";
    EmulationEnableRewind, Emulation, Controls, "Enable rewind", "history playback";
    EmulationRewindHistoryLength, Emulation, Controls, "Rewind history length", "seconds snapshots duration";
    EmulationRewindPlaybackMode, Emulation, Controls, "Rewind playback mode", "real time fast";
    EmulationFastRewindStep, Emulation, Controls, "Fast rewind step", "snapshots speed";
    EmulationGameBoyHardwareMode, Emulation, Controls, "Game Boy hardware mode", "gb dmg cgb gbc sgb auto model";
    EmulationEnableSgbBorderRendering, Emulation, Controls, "Enable SGB border rendering", "game boy super gb";
    EmulationTcpLinkAddress, Emulation, Controls, "TCP link address", "game boy gb link cable network serial host port";
    EmulationEnableNesZapper, Emulation, Controls, "Enable NES Zapper", "light gun mouse nintendo";
    EmulationPcEngineConsoleWiring, Emulation, Controls, "PC Engine console wiring", "pce turbografx16 region";
    EmulationPcEngineController, Emulation, Controls, "PC Engine controller", "pce two six 2 6 button pad multitap 5 port mouse";
    EmulationPcEngineArcadeCard, Emulation, Controls, "PC Engine Arcade Card", "pce system card v3";
    EmulationMemoryBase128, Emulation, Controls, "Memory Base 128", "pc engine pce storage accessory";
    EmulationPcEngineMouseCursor, Emulation, Controls, "PC Engine mouse cursor", "pce captured free";
    EmulationPcEngineMouseSensitivity, Emulation, Controls, "PC Engine mouse sensitivity", "pce motion";
    EmulationSegaVideoStandard, Emulation, Controls, "Sega video standard", "master system game gear sg1000 sms ntsc pal";
    EmulationSegaConsoleRegion, Emulation, Controls, "Sega console region", "master system game gear sg1000 sms japanese export pbc";
    FirmwareGameBoyBootRomStartup, Firmware, Controls, "Game Boy boot ROM startup", "gb gbc dmg cgb bios skip external firmware";
    FirmwareGbaBiosBootMode, Firmware, Controls, "GBA BIOS boot mode", "game boy advance hle external startup firmware";
    FirmwareSegaBootRomStartup, Firmware, Controls, "Sega boot ROM startup", "master system game gear sms gg bios skip external";
    FirmwareImportFirmware, Firmware, Controls, "Import firmware", "bios boot rom file fds colecovision gb gbc gba sms gg";
    #[cfg(not(target_arch = "wasm32"))]
    FirmwareScanFirmwareNow, Firmware, Controls, "Scan firmware now", "bios boot rom status missing recognized fds colecovision";
    #[cfg(not(target_arch = "wasm32"))]
    FirmwareAdditionalFirmwareSearchFolder, Firmware, Controls, "Additional firmware search folder", "bios boot rom directory browse path";
    FirmwareFirmwareFileDetails, Firmware, Controls, "Firmware file details", "bios status sha256 hash recognized missing invalid fds colecovision";
    FirmwareRemoveImportedFirmware, Firmware, Controls, "Remove imported firmware", "bios boot rom delete managed file";
    StorageSaveRecoveryStateWhenStopping, Storage, Controls, "Save recovery state when stopping", "autosave automatic save shutdown close";
    StorageResumeFreshRecoveryStateAutomatically, Storage, Controls, "Resume fresh recovery state automatically", "autoload automatic resume startup load save state";
    StorageKeepAutomaticResume, Storage, Controls, "Keep automatic resume", "migration recovery save state";
    StorageKeepResumeOff, Storage, Controls, "Keep resume off", "migration recovery automatic save state";
    CameraCameraDevice, Camera, Controls, "Camera device", "host webcam pocket game boy sensor index";
    CameraCameraDeviceIndex, Camera, Controls, "Camera device index", "advanced device selection manual host webcam";
    CameraRefreshCameraDevices, Camera, Controls, "Refresh camera devices", "host webcam enumerate detect";
    CameraAutomaticLevels, Camera, Controls, "Automatic levels", "pocket camera brightness exposure";
    CameraCameraBrightness, Camera, Controls, "Camera brightness", "pocket host tuning";
    CameraCameraContrast, Camera, Controls, "Camera contrast", "pocket host tuning";
    CameraCameraGamma, Camera, Controls, "Camera gamma", "pocket host tuning";
    CameraResetCameraTuning, Camera, Controls, "Reset camera tuning", "brightness contrast gamma default";
    InterfaceUiTheme, Interface, Controls, "UI theme", "appearance dark light high contrast retro green accessibility colors";
    InterfaceUiDensity, Interface, Controls, "UI density", "compact comfortable spacing accessibility";
    InterfaceAutohideMenuBar, Interface, Controls, "Autohide menu bar", "interface menu hide";
    InterfaceUiScale, Interface, Controls, "UI scale", "zoom dpi readability font size accessibility";
    DebuggerDebugMonospace, Debugger, Controls, "Debug monospace", "font size scale text readability";
    DebuggerUseUiThemePalette, Debugger, Controls, "Use UI theme palette", "debugger colors reset";
    DebuggerAddressColor, Debugger, Controls, "Address color", "debugger colors memory";
    DebuggerOpcodeBytesColor, Debugger, Controls, "Opcode bytes color", "debugger colors disassembly";
    DebuggerMnemonicColor, Debugger, Controls, "Mnemonic color", "debugger colors disassembly instruction";
    DebuggerSymbolColor, Debugger, Controls, "Symbol color", "debugger colors labels";
    DebuggerSourceColor, Debugger, Controls, "Source color", "debugger colors code";
    DebuggerCurrentPcColor, Debugger, Controls, "Current PC color", "debugger colors program counter";
    DebuggerChangedValueColor, Debugger, Controls, "Changed value color", "debugger colors memory";
    DebuggerBreakpointColor, Debugger, Controls, "Breakpoint color", "debugger colors execution";
    DebuggerWatchpointColor, Debugger, Controls, "Watchpoint color", "debugger colors memory";
    DebuggerSelectionColor, Debugger, Controls, "Selection color", "debugger colors highlight";
    DebuggerInterruptColor, Debugger, Controls, "Interrupt color", "debugger colors irq";
    DebuggerResetDebuggerColors, Debugger, Controls, "Reset debugger colors", "palette defaults";
    DebuggerDebuggerLayout, Debugger, Controls, "Debugger layout", "presentation window dock separate game";
    DebuggerShowFpsInDebugPanel, Debugger, Controls, "Show FPS in debug panel", "framerate performance";
    DebuggerEnableMemoryEditing, Debugger, Controls, "Enable memory editing", "memory viewer write addresses";
    InputDevicesPlayer, InputDevices, Controls, "Player", "input mapping keyboard controller p1 p2 p3 p4 p5 multitap";
    InputDevicesButtonLayout, InputDevices, Controls, "Diagram", "button layout controller guide automatic game boy gba nes snes sega pce wonderswan";
    InputDevicesInputDevice, InputDevices, Controls, "Input device", "controller gamepad automatic assignment disable reserve select";
    InputDevicesKeyboardMappings, InputDevices, Controls, "Keyboard mappings", "key bindings remap dpad up down left right a b x y l r start select";
    InputDevicesControllerMappings, InputDevices, Controls, "Controller mappings", "gamepad buttons remap dpad up down left right a b x y l r start select";
    InputDevicesWonderswanDirectMappings, InputDevices, Controls, "WonderSwan direct mappings", "ws x1 x2 x3 x4 y1 y2 y3 y4 a b start keyboard gamepad";
    InputDevicesRestoreDefaultMappings, InputDevices, Controls, "Restore default mappings", "reset keyboard controller bindings player";
    InputDevicesClearControllerMapping, InputDevices, Controls, "Clear controller mapping", "gamepad binding remove unbind";
    InputDevicesClearWonderswanDirectMappings, InputDevices, Controls, "Clear WonderSwan direct mappings", "ws gamepad bindings unbind";
    InputDevicesApplyInputProfile, InputDevices, Controls, "Apply input profile", "saved profiles bindings load select";
    InputDevicesSaveCurrentAsProfile, InputDevices, Controls, "Save current as profile", "input profiles new name keyboard gamepad";
    InputDevicesUpdateSelectedProfile, InputDevices, Controls, "Update selected profile", "input profiles save overwrite mappings";
    InputDevicesResetSelectedProfile, InputDevices, Controls, "Reset selected profile", "input profiles default bindings";
    InputDevicesDeleteSelectedProfile, InputDevices, Controls, "Delete selected profile", "input profiles remove";
    InputDevicesCancelCapture, InputDevices, Controls, "Cancel capture", "input binding remap key controller";
    InputDevicesSpeedUpKey, InputDevices, Hotkeys, "Speed-up key", "keyboard shortcut hold fast forward speedup";
    InputDevicesRewindKey, InputDevices, Hotkeys, "Rewind key", "keyboard shortcut hold";
    InputDevicesPauseResumeShortcut, InputDevices, Hotkeys, "Pause / Resume shortcut", "keyboard hotkey emulator";
    InputDevicesFullscreenShortcut, InputDevices, Hotkeys, "Fullscreen shortcut", "keyboard hotkey display";
    InputDevicesToggleSlowMotionShortcut, InputDevices, Hotkeys, "Toggle slow motion shortcut", "keyboard hotkey speed";
    InputDevicesToggleUncappedShortcut, InputDevices, Hotkeys, "Toggle uncapped shortcut", "keyboard hotkey speed";
    InputDevicesMuteToggleShortcut, InputDevices, Hotkeys, "Mute toggle shortcut", "keyboard hotkey audio sound";
    InputDevicesScreenshotShortcut, InputDevices, Hotkeys, "Screenshot shortcut", "keyboard hotkey image capture";
    InputDevicesResetGameShortcut, InputDevices, Hotkeys, "Reset game shortcut", "keyboard hotkey reboot";
    InputDevicesFrameAdvanceShortcut, InputDevices, Hotkeys, "Frame advance shortcut", "keyboard hotkey step";
    InputDevicesQuickSaveShortcut, InputDevices, Hotkeys, "Quick save shortcut", "keyboard hotkey state";
    InputDevicesQuickLoadShortcut, InputDevices, Hotkeys, "Quick load shortcut", "keyboard hotkey state";
    InputDevicesNextSaveSlotShortcut, InputDevices, Hotkeys, "Next save slot shortcut", "keyboard hotkey state bracket";
    InputDevicesPreviousSaveSlotShortcut, InputDevices, Hotkeys, "Previous save slot shortcut", "keyboard hotkey state bracket";
    InputDevicesRotateWonderswanShortcut, InputDevices, Hotkeys, "Rotate WonderSwan shortcut", "keyboard hotkey ws orientation";
    InputDevicesRunDebuggerShortcut, InputDevices, Hotkeys, "Run debugger shortcut", "keyboard hotkey continue";
    InputDevicesStepDebuggerShortcut, InputDevices, Hotkeys, "Step debugger shortcut", "keyboard hotkey instruction";
    InputDevicesGamepadSpeedUpAction, InputDevices, Hotkeys, "Gamepad speed-up action", "controller button hold fast forward";
    InputDevicesGamepadRewindAction, InputDevices, Hotkeys, "Gamepad rewind action", "controller button hold";
    InputDevicesGamepadPauseAction, InputDevices, Hotkeys, "Gamepad pause action", "controller button toggle resume";
    InputDevicesGamepadTurboAction, InputDevices, Hotkeys, "Gamepad turbo action", "controller button rapid fire hold";
    InputDevicesLeftStickDeadzone, InputDevices, TestAndCalibrate, "Left-stick deadzone", "gamepad controller calibration threshold drift dpad tilt";
    InputDevicesTestControllerButtons, InputDevices, TestAndCalibrate, "Test controller buttons", "gamepad input diagnostics pressed mapped raw";
    InputDevicesAutofireA, InputDevices, Controls, "Autofire A", "turbo rapid fire repeat button period duty emulated frame";
    InputDevicesAutofireB, InputDevices, Controls, "Autofire B", "turbo rapid fire repeat button period duty emulated frame";
    InputDevicesAutofireX, InputDevices, Controls, "Autofire X", "turbo rapid fire repeat button period duty emulated frame";
    InputDevicesAutofireY, InputDevices, Controls, "Autofire Y", "turbo rapid fire repeat button period duty emulated frame";
    InputDevicesAutofireL, InputDevices, Controls, "Autofire L", "turbo rapid fire repeat shoulder period duty emulated frame";
    InputDevicesAutofireR, InputDevices, Controls, "Autofire R", "turbo rapid fire repeat shoulder period duty emulated frame";
    InputDevicesAutofireStart, InputDevices, Controls, "Autofire Start", "turbo rapid fire repeat button period duty emulated frame";
    InputDevicesAutofireSelect, InputDevices, Controls, "Autofire Select", "turbo rapid fire repeat button period duty emulated frame";
    InputDevicesTestLeftStick, InputDevices, TestAndCalibrate, "Test left stick", "gamepad controller diagnostics axes x y raw calibration";
    InputDevicesTestRightStick, InputDevices, TestAndCalibrate, "Test right stick", "gamepad controller diagnostics axes x y raw calibration";
    InputDevicesStartCalibration, InputDevices, TestAndCalibrate, "Start calibration", "gamepad controller stick center sample calibration raw";
    InputDevicesMeasureRangeCalibration, InputDevices, TestAndCalibrate, "Measure calibration range", "gamepad controller stick min max extrema calibration";
    InputDevicesApplyCalibration, InputDevices, TestAndCalibrate, "Apply calibration", "gamepad controller stick save model calibration";
    InputDevicesCancelCalibration, InputDevices, TestAndCalibrate, "Cancel calibration", "gamepad controller stick discard calibration";
    InputDevicesResetCalibration, InputDevices, TestAndCalibrate, "Reset calibration", "gamepad controller stick remove model calibration";
    InputDevicesLeftStickBehavior, InputDevices, TestAndCalibrate, "Left stick behavior", "mbc7 tilt dpad auto";
    InputDevicesTiltInputSource, InputDevices, TestAndCalibrate, "Tilt input source", "mbc7 keyboard wasd mouse automatic";
    InputDevicesInvertTiltX, InputDevices, TestAndCalibrate, "Invert tilt X", "mbc7 horizontal axis";
    InputDevicesInvertTiltY, InputDevices, TestAndCalibrate, "Invert tilt Y", "mbc7 vertical axis";
    InputDevicesDirectLeftStickTilt, InputDevices, TestAndCalibrate, "Direct left-stick tilt", "mbc7 bypass lerp";
    InputDevicesTiltSensitivity, InputDevices, TestAndCalibrate, "Tilt sensitivity", "mbc7 motion";
    InputDevicesTiltSmoothing, InputDevices, TestAndCalibrate, "Tilt smoothing", "mbc7 lerp";
    InputDevicesTiltDeadzone, InputDevices, TestAndCalibrate, "Tilt deadzone", "mbc7 stick threshold drift";
    InputDevicesTiltKeyBindings, InputDevices, TestAndCalibrate, "Tilt key bindings", "mbc7 keyboard up down left right";
    InputDevicesResetTiltKeysToWasd, InputDevices, TestAndCalibrate, "Reset tilt keys to WASD", "mbc7 keyboard default";
}

pub(super) fn metadata(id: SettingId) -> &'static SearchEntry {
    ENTRIES
        .iter()
        .find(|entry| entry.id == id)
        .expect("registered setting")
}

pub(super) fn breadcrumb(id: SettingId) -> String {
    let entry = metadata(id);
    if entry.category == SettingsCategory::InputDevices {
        format!("{} › {}", entry.category.label(), entry.input_page.label())
    } else {
        entry.category.label().to_owned()
    }
}

#[derive(Clone, Default)]
struct Navigation {
    pending: Option<SettingId>,
    highlighted: Option<(SettingId, egui::Id, f64)>,
    notice: Option<String>,
}

fn navigation_id() -> egui::Id {
    egui::Id::new("settings_search_navigation")
}

pub(super) fn begin(ctx: &egui::Context, id: SettingId) {
    ctx.data_mut(|data| {
        data.insert_temp(
            navigation_id(),
            Navigation {
                pending: Some(id),
                ..Default::default()
            },
        );
    });
    ctx.request_repaint();
}

pub(super) fn clear(ctx: &egui::Context) {
    ctx.data_mut(|data| data.remove::<Navigation>(navigation_id()));
}

pub(super) fn requested(ui: &egui::Ui, id: SettingId) -> bool {
    ui.ctx().data(|data| {
        data.get_temp::<Navigation>(navigation_id())
            .is_some_and(|nav| nav.pending == Some(id))
    })
}

pub(super) fn notice(ui: &egui::Ui) -> Option<String> {
    ui.ctx().data(|data| {
        data.get_temp::<Navigation>(navigation_id())
            .and_then(|nav| nav.notice)
    })
}

pub(super) fn target(ui: &mut egui::Ui, id: SettingId, response: &egui::Response) {
    let now = ui.input(|input| input.time);
    let mut navigation = ui
        .ctx()
        .data(|data| data.get_temp::<Navigation>(navigation_id()))
        .unwrap_or_default();
    if navigation.pending == Some(id) {
        response.scroll_to_me(Some(egui::Align::Center));
        if response.enabled() && response.sense.is_focusable() {
            response.request_focus();
        }
        navigation.pending = None;
        navigation.highlighted = Some((id, response.id, now));
        ui.ctx()
            .data_mut(|data| data.insert_temp(navigation_id(), navigation.clone()));
    }
    if navigation
        .highlighted
        .is_some_and(|(target, widget, start)| {
            target == id && widget == response.id && now - start < 2.0
        })
    {
        ui.painter().rect_stroke(
            response.rect.expand(2.0),
            4.0,
            egui::Stroke::new(2.0, ui.visuals().text_color()),
            egui::StrokeKind::Outside,
        );
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(100));
    }
}

pub(super) fn conditional(
    ui: &egui::Ui,
    id: SettingId,
    prerequisite: SettingId,
    available: bool,
    explanation: &str,
) {
    if !available && requested(ui, id) {
        ui.ctx().data_mut(|data| {
            let mut navigation = data
                .get_temp::<Navigation>(navigation_id())
                .unwrap_or_default();
            navigation.pending = Some(prerequisite);
            navigation.notice = Some(explanation.to_owned());
            data.insert_temp(navigation_id(), navigation);
        });
        ui.ctx().request_repaint();
    }
}

pub(super) fn results(query: &str) -> Vec<&'static SearchEntry> {
    let query = query.to_lowercase();
    let tokens: Vec<_> = query.split_whitespace().collect();
    if tokens.is_empty() {
        return Vec::new();
    }
    let phrase = tokens.join(" ");
    let mut matches: Vec<_> = ENTRIES
        .iter()
        .filter_map(|entry| {
            let title = entry.title.to_lowercase();
            let keywords = entry.keywords.to_lowercase();
            let mut category = entry.category.label().to_lowercase();
            if entry.category == SettingsCategory::InputDevices {
                category.push(' ');
                category.push_str(&entry.input_page.label().to_lowercase());
            }
            if !tokens.iter().all(|token| {
                title.contains(token) || keywords.contains(token) || category.contains(token)
            }) {
                return None;
            }
            let title_matches = tokens
                .iter()
                .filter(|token| title.contains(**token))
                .count();
            let keyword_matches = tokens
                .iter()
                .filter(|token| keywords.contains(**token))
                .count();
            let rank = (
                title == phrase,
                title.contains(&phrase),
                title_matches,
                keyword_matches,
            );
            Some((rank, entry))
        })
        .collect();
    matches.sort_by(|(left_rank, left), (right_rank, right)| {
        right_rank
            .cmp(left_rank)
            .then_with(|| left.title.cmp(right.title))
    });
    matches.into_iter().map(|(_, entry)| entry).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_search_destination_resolves_without_changing_settings() {
        let mut unresolved = Vec::new();
        for (entry, has_profile) in ENTRIES
            .iter()
            .flat_map(|entry| [false, true].map(|has_profile| (entry, has_profile)))
        {
            let context = egui::Context::default();
            let mut settings = crate::settings::Settings::default();
            if has_profile {
                settings
                    .save_controller_profile(
                        "Test controller",
                        &crate::settings::InputScope::Global,
                    )
                    .unwrap();
            }
            let original = settings.clone();
            let mut state = crate::debug::DebugWindowState::new();
            state.settings_ui.category = entry.category;
            state.settings_ui.input_page = entry.input_page;
            state.settings_ui.selected_profile_id = settings
                .input_profiles
                .profiles
                .first()
                .map(|profile| profile.id.clone());
            state.camera_devices_needs_refresh = false;
            #[cfg(not(target_arch = "wasm32"))]
            {
                state.firmware_inventory.needs_refresh = false;
            }
            super::super::controls::prepare_search_target(&mut state, entry.id);
            begin(&context, entry.id);
            for frame in 0..16 {
                let _ = context.run_ui(
                    egui::RawInput {
                        time: Some(frame as f64 / 30.0),
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(1100.0, 900.0),
                        )),
                        ..Default::default()
                    },
                    |ui| {
                        super::super::draw_settings_content(
                            ui,
                            &mut settings,
                            &mut state,
                            &super::super::SettingsContext {
                                active_system: None,
                                gb_hardware_mode_label: None,
                                is_pocket_camera: false,
                                #[cfg(target_arch = "wasm32")]
                                nes_palette_file_slot: crate::platform::FileDataSlot::default(),
                            },
                        )
                    },
                );
            }
            let nav = context
                .data(|data| data.get_temp::<Navigation>(navigation_id()))
                .unwrap();
            if nav.pending.is_some() {
                unresolved.push((entry.id, has_profile, nav.pending));
            }
            assert_eq!(
                settings, original,
                "search mutated settings: {:?}",
                entry.id
            );
        }
        assert!(
            unresolved.is_empty(),
            "unresolved search targets: {unresolved:?}"
        );
    }

    #[test]
    fn conditional_search_redirects_to_prerequisite_then_focuses_it() {
        let context = egui::Context::default();
        begin(&context, SettingId::AudioLowPassCutoff);
        let mut target_id = None;
        let _ = context.run_ui(egui::RawInput::default(), |ui| {
            conditional(
                ui,
                SettingId::AudioLowPassCutoff,
                SettingId::AudioEnableLowPassOutputFilter,
                false,
                "Enable the filter to adjust its cutoff.",
            );
            let mut enabled = false;
            let response = ui.checkbox(&mut enabled, "Filter");
            target_id = Some(response.id);
            target(ui, SettingId::AudioEnableLowPassOutputFilter, &response);
            assert!(!enabled);
        });
        let nav = context
            .data(|data| data.get_temp::<Navigation>(navigation_id()))
            .unwrap();
        assert!(nav.pending.is_none());
        assert_eq!(
            nav.highlighted.map(|(id, _, _)| id),
            Some(SettingId::AudioEnableLowPassOutputFilter)
        );
        assert!(nav.notice.unwrap().contains("cutoff"));
        assert_eq!(context.memory(|memory| memory.focused()), target_id);
    }

    #[test]
    fn empty_and_unmatched_queries_have_no_results() {
        assert!(results(" \t\n ").is_empty());
        assert!(results("not-an-existing-setting").is_empty());
        assert_eq!(
            results("per-game override")[0].id,
            SettingId::InputDevicesMappingScope
        );
    }

    #[test]
    fn tokens_match_across_fields_without_case_or_order_dependence() {
        let found = results("  AUDIO   volume ");
        assert_eq!(found[0].title, "Master volume");
        assert_eq!(results("volume audio")[0].title, found[0].title);
        assert!(results("audio volume nonexistent").is_empty());
        assert_eq!(results("BIOS advance")[0].title, "GBA BIOS boot mode");
    }

    #[test]
    fn titles_rank_above_broad_categories_and_keywords() {
        assert_eq!(results("camera")[0].title, "Camera brightness");
        assert_eq!(results("camera device")[0].title, "Camera device");
        let camera_results = results("camera");
        let broad_match = camera_results
            .iter()
            .position(|entry| entry.title == "Automatic levels")
            .unwrap();
        let title_match = camera_results
            .iter()
            .position(|entry| entry.title == "Reset camera tuning")
            .unwrap();
        assert!(title_match < broad_match);
        assert_eq!(results("Master volume")[0].title, "Master volume");
    }

    #[test]
    fn input_results_target_the_correct_workspace() {
        for (query, page) in [
            ("input profile", InputDevicesPage::Controls),
            ("quick save", InputDevicesPage::Hotkeys),
            ("right stick", InputDevicesPage::TestAndCalibrate),
            ("tilt smoothing", InputDevicesPage::TestAndCalibrate),
        ] {
            let entry = results(query)[0];
            assert_eq!(entry.category, SettingsCategory::InputDevices);
            assert_eq!(entry.input_page, page);
        }
    }

    #[test]
    fn every_page_and_conditional_controls_are_indexed() {
        for category in SettingsCategory::ALL {
            assert!(ENTRIES.iter().any(|entry| entry.category == category));
        }
        for (query, category) in [
            ("shader curvature", SettingsCategory::Video),
            ("gbc matrix", SettingsCategory::Video),
            ("firmware import", SettingsCategory::Firmware),
            ("recovery resume", SettingsCategory::Storage),
            ("opcode color", SettingsCategory::Debugger),
            ("arcade card", SettingsCategory::Emulation),
        ] {
            assert!(
                results(query)
                    .iter()
                    .any(|entry| entry.category == category),
                "{query}"
            );
        }
    }

    #[test]
    fn index_has_no_duplicate_destinations_or_empty_names() {
        for (index, entry) in ENTRIES.iter().enumerate() {
            assert!(!entry.title.trim().is_empty());
            assert!(
                !ENTRIES[..index]
                    .iter()
                    .any(|other| other.title == entry.title
                        && other.category == entry.category
                        && other.input_page == entry.input_page)
            );
        }
    }
}
