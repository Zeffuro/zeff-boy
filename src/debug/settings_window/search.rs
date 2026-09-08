use super::{InputDevicesPage, SettingsCategory};

pub(super) struct SearchEntry {
    pub(super) title: &'static str,
    pub(super) category: SettingsCategory,
    pub(super) input_page: InputDevicesPage,
    pub(super) keywords: &'static str,
}

const fn entry(
    category: SettingsCategory,
    input_page: InputDevicesPage,
    title: &'static str,
    keywords: &'static str,
) -> SearchEntry {
    SearchEntry {
        title,
        category,
        input_page,
        keywords,
    }
}

const ENTRIES: &[SearchEntry] = &[
    entry(
        SettingsCategory::InputDevices,
        InputDevicesPage::Controls,
        "D-pad Up mapping",
        "up direction keyboard key controller gamepad binding",
    ),
    entry(
        SettingsCategory::InputDevices,
        InputDevicesPage::Controls,
        "D-pad Down mapping",
        "down direction keyboard key controller gamepad binding",
    ),
    entry(
        SettingsCategory::InputDevices,
        InputDevicesPage::Controls,
        "D-pad Left mapping",
        "left direction keyboard key controller gamepad binding",
    ),
    entry(
        SettingsCategory::InputDevices,
        InputDevicesPage::Controls,
        "D-pad Right mapping",
        "right direction keyboard key controller gamepad binding",
    ),
    entry(
        SettingsCategory::InputDevices,
        InputDevicesPage::Controls,
        "A button mapping",
        "keyboard key controller gamepad binding",
    ),
    entry(
        SettingsCategory::InputDevices,
        InputDevicesPage::Controls,
        "B button mapping",
        "keyboard key controller gamepad binding",
    ),
    entry(
        SettingsCategory::InputDevices,
        InputDevicesPage::Controls,
        "X button mapping",
        "snes pce keyboard key controller gamepad binding",
    ),
    entry(
        SettingsCategory::InputDevices,
        InputDevicesPage::Controls,
        "Y button mapping",
        "snes pce keyboard key controller gamepad binding",
    ),
    entry(
        SettingsCategory::InputDevices,
        InputDevicesPage::Controls,
        "L shoulder mapping",
        "gba snes keyboard key controller gamepad binding",
    ),
    entry(
        SettingsCategory::InputDevices,
        InputDevicesPage::Controls,
        "R shoulder mapping",
        "gba snes keyboard key controller gamepad binding",
    ),
    entry(
        SettingsCategory::InputDevices,
        InputDevicesPage::Controls,
        "Start button mapping",
        "keyboard key controller gamepad binding",
    ),
    entry(
        SettingsCategory::InputDevices,
        InputDevicesPage::Controls,
        "Select button mapping",
        "keyboard key controller gamepad binding",
    ),
    entry(
        SettingsCategory::InputDevices,
        InputDevicesPage::Controls,
        "WonderSwan X1 mapping",
        "ws direct keyboard key controller gamepad binding",
    ),
    entry(
        SettingsCategory::InputDevices,
        InputDevicesPage::Controls,
        "WonderSwan X2 mapping",
        "ws direct keyboard key controller gamepad binding",
    ),
    entry(
        SettingsCategory::InputDevices,
        InputDevicesPage::Controls,
        "WonderSwan X3 mapping",
        "ws direct keyboard key controller gamepad binding",
    ),
    entry(
        SettingsCategory::InputDevices,
        InputDevicesPage::Controls,
        "WonderSwan X4 mapping",
        "ws direct keyboard key controller gamepad binding",
    ),
    entry(
        SettingsCategory::InputDevices,
        InputDevicesPage::Controls,
        "WonderSwan Y1 mapping",
        "ws direct keyboard key controller gamepad binding",
    ),
    entry(
        SettingsCategory::InputDevices,
        InputDevicesPage::Controls,
        "WonderSwan Y2 mapping",
        "ws direct keyboard key controller gamepad binding",
    ),
    entry(
        SettingsCategory::InputDevices,
        InputDevicesPage::Controls,
        "WonderSwan Y3 mapping",
        "ws direct keyboard key controller gamepad binding",
    ),
    entry(
        SettingsCategory::InputDevices,
        InputDevicesPage::Controls,
        "WonderSwan Y4 mapping",
        "ws direct keyboard key controller gamepad binding",
    ),
    entry(
        SettingsCategory::InputDevices,
        InputDevicesPage::Controls,
        "WonderSwan A mapping",
        "ws direct keyboard key controller gamepad binding",
    ),
    entry(
        SettingsCategory::InputDevices,
        InputDevicesPage::Controls,
        "WonderSwan B mapping",
        "ws direct keyboard key controller gamepad binding",
    ),
    entry(
        SettingsCategory::InputDevices,
        InputDevicesPage::Controls,
        "WonderSwan Start mapping",
        "ws direct keyboard key controller gamepad binding",
    ),
    entry(
        SettingsCategory::InputDevices,
        InputDevicesPage::TestAndCalibrate,
        "Tilt Up key",
        "mbc7 keyboard tilt binding",
    ),
    entry(
        SettingsCategory::InputDevices,
        InputDevicesPage::TestAndCalibrate,
        "Tilt Down key",
        "mbc7 keyboard tilt binding",
    ),
    entry(
        SettingsCategory::InputDevices,
        InputDevicesPage::TestAndCalibrate,
        "Tilt Left key",
        "mbc7 keyboard tilt binding",
    ),
    entry(
        SettingsCategory::InputDevices,
        InputDevicesPage::TestAndCalibrate,
        "Tilt Right key",
        "mbc7 keyboard tilt binding",
    ),
    entry(
        SettingsCategory::General,
        InputDevicesPage::Controls,
        "Pause when window loses focus",
        "background inactive focus pause",
    ),
    #[cfg(not(target_arch = "wasm32"))]
    entry(
        SettingsCategory::General,
        InputDevicesPage::Controls,
        "Check for updates on startup",
        "version upgrade launch startup",
    ),
    entry(
        SettingsCategory::General,
        InputDevicesPage::Controls,
        "Clear recent content",
        "history recent rom games files list",
    ),
    entry(
        SettingsCategory::General,
        InputDevicesPage::Controls,
        "Reset all preferences",
        "defaults restore settings",
    ),
    entry(
        SettingsCategory::Storage,
        InputDevicesPage::Controls,
        "Retry saving settings",
        "persistence error recovery save preferences",
    ),
    entry(
        SettingsCategory::Audio,
        InputDevicesPage::Controls,
        "Master volume",
        "sound loudness mute",
    ),
    entry(
        SettingsCategory::Audio,
        InputDevicesPage::Controls,
        "Mute audio while fast-forward is held",
        "sound speedup fast forward",
    ),
    entry(
        SettingsCategory::Audio,
        InputDevicesPage::Controls,
        "Emulator sample rate",
        "output sound hz resampling 32000 44100 48000 96000 192000",
    ),
    entry(
        SettingsCategory::Audio,
        InputDevicesPage::Controls,
        "Enable low-pass output filter",
        "sound low pass filtering",
    ),
    entry(
        SettingsCategory::Audio,
        InputDevicesPage::Controls,
        "Low-pass cutoff",
        "filter frequency hz",
    ),
    entry(
        SettingsCategory::Audio,
        InputDevicesPage::Controls,
        "Recording format",
        "sound wav ogg vorbis pcm",
    ),
    entry(
        SettingsCategory::Video,
        InputDevicesPage::Controls,
        "VSync",
        "vertical synchronization tearing adaptive",
    ),
    entry(
        SettingsCategory::Video,
        InputDevicesPage::Controls,
        "Scaling mode",
        "nearest linear hq2x xbr2x eagle2x upscale pixel",
    ),
    entry(
        SettingsCategory::Video,
        InputDevicesPage::Controls,
        "Offscreen scale",
        "resolution render target size",
    ),
    entry(
        SettingsCategory::Video,
        InputDevicesPage::Controls,
        "Edge strength",
        "shader scaling upscaler hq2x xbr2x eagle2x",
    ),
    entry(
        SettingsCategory::Video,
        InputDevicesPage::Controls,
        "Effect",
        "shader scanlines lcd grid crt gbc palette custom",
    ),
    entry(
        SettingsCategory::Video,
        InputDevicesPage::Controls,
        "Scanline intensity",
        "shader effect scanlines crt",
    ),
    entry(
        SettingsCategory::Video,
        InputDevicesPage::Controls,
        "LCD grid intensity",
        "shader effect lcd grid",
    ),
    entry(
        SettingsCategory::Video,
        InputDevicesPage::Controls,
        "CRT curvature",
        "shader effect crt curved",
    ),
    entry(
        SettingsCategory::Video,
        InputDevicesPage::Controls,
        "Palette mix",
        "shader effect gbc palette",
    ),
    entry(
        SettingsCategory::Video,
        InputDevicesPage::Controls,
        "Palette warmth",
        "shader effect gbc palette",
    ),
    entry(
        SettingsCategory::Video,
        InputDevicesPage::Controls,
        "Load custom WGSL shader",
        "effect fragment file path import",
    ),
    entry(
        SettingsCategory::Video,
        InputDevicesPage::Controls,
        "Clear custom shader",
        "effect wgsl fragment path reset",
    ),
    entry(
        SettingsCategory::Video,
        InputDevicesPage::Controls,
        "GB/GBC color correction",
        "game boy color custom matrix lcd",
    ),
    entry(
        SettingsCategory::Video,
        InputDevicesPage::Controls,
        "GB/GBC custom color matrix",
        "game boy color correction rgb identity gbc matrix",
    ),
    entry(
        SettingsCategory::Video,
        InputDevicesPage::Controls,
        "DMG palette",
        "game boy monochrome green grayscale",
    ),
    entry(
        SettingsCategory::Video,
        InputDevicesPage::Controls,
        "GBA color correction",
        "game boy advance lcd custom",
    ),
    entry(
        SettingsCategory::Video,
        InputDevicesPage::Controls,
        "GBA custom color matrix",
        "game boy advance rgb correction identity",
    ),
    entry(
        SettingsCategory::Video,
        InputDevicesPage::Controls,
        "WS color correction",
        "wonderswan swancrystal lcd custom",
    ),
    entry(
        SettingsCategory::Video,
        InputDevicesPage::Controls,
        "WonderSwan custom color matrix",
        "ws wsc lcd rgb correction identity",
    ),
    entry(
        SettingsCategory::Video,
        InputDevicesPage::Controls,
        "NES palette mode",
        "nintendo colors custom pal",
    ),
    entry(
        SettingsCategory::Video,
        InputDevicesPage::Controls,
        "Load NES palette file",
        "custom nes .pal import",
    ),
    entry(
        SettingsCategory::Video,
        InputDevicesPage::Controls,
        "Clear NES palette file",
        "custom nes .pal reset",
    ),
    entry(
        SettingsCategory::Video,
        InputDevicesPage::Controls,
        "PC Engine visible area",
        "pce overscan crop display",
    ),
    entry(
        SettingsCategory::Video,
        InputDevicesPage::Controls,
        "PC Engine color output",
        "pce palette rgb composite",
    ),
    entry(
        SettingsCategory::Emulation,
        InputDevicesPage::Controls,
        "Fast-forward multiplier",
        "speed speedup fast forward",
    ),
    entry(
        SettingsCategory::Emulation,
        InputDevicesPage::Controls,
        "Start in slow-motion mode",
        "speed slow motion",
    ),
    entry(
        SettingsCategory::Emulation,
        InputDevicesPage::Controls,
        "Slow-motion divisor",
        "speed slow motion",
    ),
    entry(
        SettingsCategory::Emulation,
        InputDevicesPage::Controls,
        "Uncapped frames/tick",
        "speed throughput latency batch",
    ),
    entry(
        SettingsCategory::Emulation,
        InputDevicesPage::Controls,
        "Start in uncapped mode",
        "speed unlimited",
    ),
    entry(
        SettingsCategory::Emulation,
        InputDevicesPage::Controls,
        "Frame skip when behind",
        "timing speed frameskip",
    ),
    entry(
        SettingsCategory::Emulation,
        InputDevicesPage::Controls,
        "7z decoder memory limit",
        "archive pc engine pce cd ram",
    ),
    entry(
        SettingsCategory::Emulation,
        InputDevicesPage::Controls,
        "Enable rewind",
        "history playback",
    ),
    entry(
        SettingsCategory::Emulation,
        InputDevicesPage::Controls,
        "Rewind history length",
        "seconds snapshots duration",
    ),
    entry(
        SettingsCategory::Emulation,
        InputDevicesPage::Controls,
        "Rewind playback mode",
        "real time fast",
    ),
    entry(
        SettingsCategory::Emulation,
        InputDevicesPage::Controls,
        "Fast rewind step",
        "snapshots speed",
    ),
    entry(
        SettingsCategory::Emulation,
        InputDevicesPage::Controls,
        "Game Boy hardware mode",
        "gb dmg cgb gbc sgb auto model",
    ),
    entry(
        SettingsCategory::Emulation,
        InputDevicesPage::Controls,
        "Enable SGB border rendering",
        "game boy super gb",
    ),
    entry(
        SettingsCategory::Emulation,
        InputDevicesPage::Controls,
        "TCP link address",
        "game boy gb link cable network serial host port",
    ),
    entry(
        SettingsCategory::Emulation,
        InputDevicesPage::Controls,
        "Enable NES Zapper",
        "light gun mouse nintendo",
    ),
    entry(
        SettingsCategory::Emulation,
        InputDevicesPage::Controls,
        "PC Engine console wiring",
        "pce turbografx16 region",
    ),
    entry(
        SettingsCategory::Emulation,
        InputDevicesPage::Controls,
        "PC Engine controller",
        "pce two six 2 6 button pad multitap 5 port mouse",
    ),
    entry(
        SettingsCategory::Emulation,
        InputDevicesPage::Controls,
        "PC Engine Arcade Card",
        "pce system card v3",
    ),
    entry(
        SettingsCategory::Emulation,
        InputDevicesPage::Controls,
        "Memory Base 128",
        "pc engine pce storage accessory",
    ),
    entry(
        SettingsCategory::Emulation,
        InputDevicesPage::Controls,
        "PC Engine mouse cursor",
        "pce captured free",
    ),
    entry(
        SettingsCategory::Emulation,
        InputDevicesPage::Controls,
        "PC Engine mouse sensitivity",
        "pce motion",
    ),
    entry(
        SettingsCategory::Emulation,
        InputDevicesPage::Controls,
        "Sega video standard",
        "master system game gear sg1000 sms ntsc pal",
    ),
    entry(
        SettingsCategory::Emulation,
        InputDevicesPage::Controls,
        "Sega console region",
        "master system game gear sg1000 sms japanese export pbc",
    ),
    entry(
        SettingsCategory::Firmware,
        InputDevicesPage::Controls,
        "Game Boy boot ROM startup",
        "gb gbc dmg cgb bios skip external firmware",
    ),
    entry(
        SettingsCategory::Firmware,
        InputDevicesPage::Controls,
        "GBA BIOS boot mode",
        "game boy advance hle external startup firmware",
    ),
    entry(
        SettingsCategory::Firmware,
        InputDevicesPage::Controls,
        "Sega boot ROM startup",
        "master system game gear sms gg bios skip external",
    ),
    entry(
        SettingsCategory::Firmware,
        InputDevicesPage::Controls,
        "Import firmware",
        "bios boot rom file fds colecovision gb gbc gba sms gg",
    ),
    #[cfg(not(target_arch = "wasm32"))]
    entry(
        SettingsCategory::Firmware,
        InputDevicesPage::Controls,
        "Scan firmware now",
        "bios boot rom status missing recognized fds colecovision",
    ),
    #[cfg(not(target_arch = "wasm32"))]
    entry(
        SettingsCategory::Firmware,
        InputDevicesPage::Controls,
        "Additional firmware search folder",
        "bios boot rom directory browse path",
    ),
    entry(
        SettingsCategory::Firmware,
        InputDevicesPage::Controls,
        "Firmware file details",
        "bios status sha256 hash recognized missing invalid fds colecovision",
    ),
    entry(
        SettingsCategory::Firmware,
        InputDevicesPage::Controls,
        "Remove imported firmware",
        "bios boot rom delete managed file",
    ),
    entry(
        SettingsCategory::Storage,
        InputDevicesPage::Controls,
        "Save recovery state when stopping",
        "autosave automatic save shutdown close",
    ),
    entry(
        SettingsCategory::Storage,
        InputDevicesPage::Controls,
        "Resume fresh recovery state automatically",
        "autoload automatic resume startup load save state",
    ),
    entry(
        SettingsCategory::Storage,
        InputDevicesPage::Controls,
        "Keep automatic resume",
        "migration recovery save state",
    ),
    entry(
        SettingsCategory::Storage,
        InputDevicesPage::Controls,
        "Keep resume off",
        "migration recovery automatic save state",
    ),
    entry(
        SettingsCategory::Camera,
        InputDevicesPage::Controls,
        "Camera device",
        "host webcam pocket game boy sensor index",
    ),
    entry(
        SettingsCategory::Camera,
        InputDevicesPage::Controls,
        "Camera device index",
        "advanced device selection manual host webcam",
    ),
    entry(
        SettingsCategory::Camera,
        InputDevicesPage::Controls,
        "Refresh camera devices",
        "host webcam enumerate detect",
    ),
    entry(
        SettingsCategory::Camera,
        InputDevicesPage::Controls,
        "Automatic levels",
        "pocket camera brightness exposure",
    ),
    entry(
        SettingsCategory::Camera,
        InputDevicesPage::Controls,
        "Camera brightness",
        "pocket host tuning",
    ),
    entry(
        SettingsCategory::Camera,
        InputDevicesPage::Controls,
        "Camera contrast",
        "pocket host tuning",
    ),
    entry(
        SettingsCategory::Camera,
        InputDevicesPage::Controls,
        "Camera gamma",
        "pocket host tuning",
    ),
    entry(
        SettingsCategory::Camera,
        InputDevicesPage::Controls,
        "Reset camera tuning",
        "brightness contrast gamma default",
    ),
    entry(
        SettingsCategory::Interface,
        InputDevicesPage::Controls,
        "UI theme",
        "appearance dark light high contrast retro green accessibility colors",
    ),
    entry(
        SettingsCategory::Interface,
        InputDevicesPage::Controls,
        "UI density",
        "compact comfortable spacing accessibility",
    ),
    entry(
        SettingsCategory::Interface,
        InputDevicesPage::Controls,
        "Autohide menu bar",
        "interface menu hide",
    ),
    entry(
        SettingsCategory::Interface,
        InputDevicesPage::Controls,
        "UI scale",
        "zoom dpi readability font size accessibility",
    ),
    entry(
        SettingsCategory::Debugger,
        InputDevicesPage::Controls,
        "Debug monospace",
        "font size scale text readability",
    ),
    entry(
        SettingsCategory::Debugger,
        InputDevicesPage::Controls,
        "Use UI theme palette",
        "debugger colors reset",
    ),
    entry(
        SettingsCategory::Debugger,
        InputDevicesPage::Controls,
        "Address color",
        "debugger colors memory",
    ),
    entry(
        SettingsCategory::Debugger,
        InputDevicesPage::Controls,
        "Opcode bytes color",
        "debugger colors disassembly",
    ),
    entry(
        SettingsCategory::Debugger,
        InputDevicesPage::Controls,
        "Mnemonic color",
        "debugger colors disassembly instruction",
    ),
    entry(
        SettingsCategory::Debugger,
        InputDevicesPage::Controls,
        "Symbol color",
        "debugger colors labels",
    ),
    entry(
        SettingsCategory::Debugger,
        InputDevicesPage::Controls,
        "Source color",
        "debugger colors code",
    ),
    entry(
        SettingsCategory::Debugger,
        InputDevicesPage::Controls,
        "Current PC color",
        "debugger colors program counter",
    ),
    entry(
        SettingsCategory::Debugger,
        InputDevicesPage::Controls,
        "Changed value color",
        "debugger colors memory",
    ),
    entry(
        SettingsCategory::Debugger,
        InputDevicesPage::Controls,
        "Breakpoint color",
        "debugger colors execution",
    ),
    entry(
        SettingsCategory::Debugger,
        InputDevicesPage::Controls,
        "Watchpoint color",
        "debugger colors memory",
    ),
    entry(
        SettingsCategory::Debugger,
        InputDevicesPage::Controls,
        "Selection color",
        "debugger colors highlight",
    ),
    entry(
        SettingsCategory::Debugger,
        InputDevicesPage::Controls,
        "Interrupt color",
        "debugger colors irq",
    ),
    entry(
        SettingsCategory::Debugger,
        InputDevicesPage::Controls,
        "Reset debugger colors",
        "palette defaults",
    ),
    entry(
        SettingsCategory::Debugger,
        InputDevicesPage::Controls,
        "Debugger layout",
        "presentation window dock separate game",
    ),
    entry(
        SettingsCategory::Debugger,
        InputDevicesPage::Controls,
        "Show FPS in debug panel",
        "framerate performance",
    ),
    entry(
        SettingsCategory::Debugger,
        InputDevicesPage::Controls,
        "Enable memory editing",
        "memory viewer write addresses",
    ),
    entry(
        SettingsCategory::InputDevices,
        InputDevicesPage::Controls,
        "Player",
        "input mapping keyboard controller p1 p2 p3 p4 p5 multitap",
    ),
    entry(
        SettingsCategory::InputDevices,
        InputDevicesPage::Controls,
        "Button layout",
        "controller guide automatic game boy gba nes snes sega pce wonderswan",
    ),
    entry(
        SettingsCategory::InputDevices,
        InputDevicesPage::Controls,
        "Input device",
        "controller gamepad automatic assignment disable reserve select",
    ),
    entry(
        SettingsCategory::InputDevices,
        InputDevicesPage::Controls,
        "Keyboard mappings",
        "key bindings remap dpad up down left right a b x y l r start select",
    ),
    entry(
        SettingsCategory::InputDevices,
        InputDevicesPage::Controls,
        "Controller mappings",
        "gamepad buttons remap dpad up down left right a b x y l r start select",
    ),
    entry(
        SettingsCategory::InputDevices,
        InputDevicesPage::Controls,
        "WonderSwan direct mappings",
        "ws x1 x2 x3 x4 y1 y2 y3 y4 a b start keyboard gamepad",
    ),
    entry(
        SettingsCategory::InputDevices,
        InputDevicesPage::Controls,
        "Restore default mappings",
        "reset keyboard controller bindings player",
    ),
    entry(
        SettingsCategory::InputDevices,
        InputDevicesPage::Controls,
        "Clear controller mapping",
        "gamepad binding remove unbind",
    ),
    entry(
        SettingsCategory::InputDevices,
        InputDevicesPage::Controls,
        "Clear WonderSwan direct mappings",
        "ws gamepad bindings unbind",
    ),
    entry(
        SettingsCategory::InputDevices,
        InputDevicesPage::Controls,
        "Apply input profile",
        "saved profiles bindings load select",
    ),
    entry(
        SettingsCategory::InputDevices,
        InputDevicesPage::Controls,
        "Save current as profile",
        "input profiles new name keyboard gamepad",
    ),
    entry(
        SettingsCategory::InputDevices,
        InputDevicesPage::Controls,
        "Update selected profile",
        "input profiles save overwrite mappings",
    ),
    entry(
        SettingsCategory::InputDevices,
        InputDevicesPage::Controls,
        "Reset selected profile",
        "input profiles default bindings",
    ),
    entry(
        SettingsCategory::InputDevices,
        InputDevicesPage::Controls,
        "Delete selected profile",
        "input profiles remove",
    ),
    entry(
        SettingsCategory::InputDevices,
        InputDevicesPage::Controls,
        "Cancel capture",
        "input binding remap key controller",
    ),
    entry(
        SettingsCategory::InputDevices,
        InputDevicesPage::Hotkeys,
        "Speed-up key",
        "keyboard shortcut hold fast forward speedup",
    ),
    entry(
        SettingsCategory::InputDevices,
        InputDevicesPage::Hotkeys,
        "Rewind key",
        "keyboard shortcut hold",
    ),
    entry(
        SettingsCategory::InputDevices,
        InputDevicesPage::Hotkeys,
        "Pause / Resume shortcut",
        "keyboard hotkey emulator",
    ),
    entry(
        SettingsCategory::InputDevices,
        InputDevicesPage::Hotkeys,
        "Fullscreen shortcut",
        "keyboard hotkey display",
    ),
    entry(
        SettingsCategory::InputDevices,
        InputDevicesPage::Hotkeys,
        "Toggle slow motion shortcut",
        "keyboard hotkey speed",
    ),
    entry(
        SettingsCategory::InputDevices,
        InputDevicesPage::Hotkeys,
        "Toggle uncapped shortcut",
        "keyboard hotkey speed",
    ),
    entry(
        SettingsCategory::InputDevices,
        InputDevicesPage::Hotkeys,
        "Mute toggle shortcut",
        "keyboard hotkey audio sound",
    ),
    entry(
        SettingsCategory::InputDevices,
        InputDevicesPage::Hotkeys,
        "Screenshot shortcut",
        "keyboard hotkey image capture",
    ),
    entry(
        SettingsCategory::InputDevices,
        InputDevicesPage::Hotkeys,
        "Reset game shortcut",
        "keyboard hotkey reboot",
    ),
    entry(
        SettingsCategory::InputDevices,
        InputDevicesPage::Hotkeys,
        "Frame advance shortcut",
        "keyboard hotkey step",
    ),
    entry(
        SettingsCategory::InputDevices,
        InputDevicesPage::Hotkeys,
        "Quick save shortcut",
        "keyboard hotkey state",
    ),
    entry(
        SettingsCategory::InputDevices,
        InputDevicesPage::Hotkeys,
        "Quick load shortcut",
        "keyboard hotkey state",
    ),
    entry(
        SettingsCategory::InputDevices,
        InputDevicesPage::Hotkeys,
        "Next save slot shortcut",
        "keyboard hotkey state bracket",
    ),
    entry(
        SettingsCategory::InputDevices,
        InputDevicesPage::Hotkeys,
        "Previous save slot shortcut",
        "keyboard hotkey state bracket",
    ),
    entry(
        SettingsCategory::InputDevices,
        InputDevicesPage::Hotkeys,
        "Rotate WonderSwan shortcut",
        "keyboard hotkey ws orientation",
    ),
    entry(
        SettingsCategory::InputDevices,
        InputDevicesPage::Hotkeys,
        "Run debugger shortcut",
        "keyboard hotkey continue",
    ),
    entry(
        SettingsCategory::InputDevices,
        InputDevicesPage::Hotkeys,
        "Step debugger shortcut",
        "keyboard hotkey instruction",
    ),
    entry(
        SettingsCategory::InputDevices,
        InputDevicesPage::Hotkeys,
        "Gamepad speed-up action",
        "controller button hold fast forward",
    ),
    entry(
        SettingsCategory::InputDevices,
        InputDevicesPage::Hotkeys,
        "Gamepad rewind action",
        "controller button hold",
    ),
    entry(
        SettingsCategory::InputDevices,
        InputDevicesPage::Hotkeys,
        "Gamepad pause action",
        "controller button toggle resume",
    ),
    entry(
        SettingsCategory::InputDevices,
        InputDevicesPage::Hotkeys,
        "Gamepad turbo action",
        "controller button rapid fire hold",
    ),
    entry(
        SettingsCategory::InputDevices,
        InputDevicesPage::TestAndCalibrate,
        "Left-stick deadzone",
        "gamepad controller calibration threshold drift dpad tilt",
    ),
    entry(
        SettingsCategory::InputDevices,
        InputDevicesPage::TestAndCalibrate,
        "Test controller buttons",
        "gamepad input diagnostics pressed mapped raw",
    ),
    entry(
        SettingsCategory::InputDevices,
        InputDevicesPage::TestAndCalibrate,
        "Test left stick",
        "gamepad controller diagnostics axes x y raw calibration",
    ),
    entry(
        SettingsCategory::InputDevices,
        InputDevicesPage::TestAndCalibrate,
        "Test right stick",
        "gamepad controller diagnostics axes x y raw calibration",
    ),
    entry(
        SettingsCategory::InputDevices,
        InputDevicesPage::TestAndCalibrate,
        "Left stick behavior",
        "mbc7 tilt dpad auto",
    ),
    entry(
        SettingsCategory::InputDevices,
        InputDevicesPage::TestAndCalibrate,
        "Tilt input source",
        "mbc7 keyboard wasd mouse automatic",
    ),
    entry(
        SettingsCategory::InputDevices,
        InputDevicesPage::TestAndCalibrate,
        "Invert tilt X",
        "mbc7 horizontal axis",
    ),
    entry(
        SettingsCategory::InputDevices,
        InputDevicesPage::TestAndCalibrate,
        "Invert tilt Y",
        "mbc7 vertical axis",
    ),
    entry(
        SettingsCategory::InputDevices,
        InputDevicesPage::TestAndCalibrate,
        "Direct left-stick tilt",
        "mbc7 bypass lerp",
    ),
    entry(
        SettingsCategory::InputDevices,
        InputDevicesPage::TestAndCalibrate,
        "Tilt sensitivity",
        "mbc7 motion",
    ),
    entry(
        SettingsCategory::InputDevices,
        InputDevicesPage::TestAndCalibrate,
        "Tilt smoothing",
        "mbc7 lerp",
    ),
    entry(
        SettingsCategory::InputDevices,
        InputDevicesPage::TestAndCalibrate,
        "Tilt deadzone",
        "mbc7 stick threshold drift",
    ),
    entry(
        SettingsCategory::InputDevices,
        InputDevicesPage::TestAndCalibrate,
        "Tilt key bindings",
        "mbc7 keyboard up down left right",
    ),
    entry(
        SettingsCategory::InputDevices,
        InputDevicesPage::TestAndCalibrate,
        "Reset tilt keys to WASD",
        "mbc7 keyboard default",
    ),
];

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
    fn empty_and_unmatched_queries_have_no_results() {
        assert!(results(" \t\n ").is_empty());
        assert!(results("not-an-existing-setting").is_empty());
        assert!(results("per-game override").is_empty());
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
