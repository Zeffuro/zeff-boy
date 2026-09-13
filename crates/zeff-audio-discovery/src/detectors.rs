use serde::Serialize;
use zeff_emu_common::system::System;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct DetectorDescriptor {
    pub id: &'static str,
    pub semantic_version: u32,
    pub scope: &'static str,
}

const MODULES: DetectorDescriptor = DetectorDescriptor {
    id: "tracker-structure",
    semantic_version: 1,
    scope: "XM 1.04, 31-instrument MOD, supported S3M/IT structures; not native console drivers",
};
pub const TRACKER: &[DetectorDescriptor] = &[MODULES];
pub const GBS: &[DetectorDescriptor] = &[DetectorDescriptor {
    id: "gbs-container",
    semantic_version: 2,
    scope: "Standalone GBS v1 header, program mapping and source preservation; no guest execution",
}];
pub const NSF: &[DetectorDescriptor] = &[DetectorDescriptor {
    id: "nsf-container",
    semantic_version: 2,
    scope: "Standalone NSF v1 header, program mapping and source preservation; no guest execution",
}];
pub const VGM: &[DetectorDescriptor] = &[DetectorDescriptor {
    id: "vgm-register-log",
    semantic_version: 1,
    scope: "Standalone VGM/VGZ command structure and source preservation; not playback or native ROM discovery",
}];
pub const CDDA: &[DetectorDescriptor] = &[DetectorDescriptor {
    id: "pce-cdda-toc",
    semantic_version: 1,
    scope: "PCE-CD audio tracks from the loaded table of contents",
}];

pub fn cartridge(system: System) -> &'static [DetectorDescriptor] {
    match system {
        System::Gba => &[
            MODULES,
            DetectorDescriptor {
                id: "gax3-structure",
                semantic_version: 4,
                scope: "GAX 3 version marker and supported song/instrument/sample graph",
            },
            DetectorDescriptor {
                id: "mp2k-sequence",
                semantic_version: 2,
                scope: "MP2k and verified MP2k song-ID headers, table evidence, referenced instruments and Camelot recipes",
            },
            DetectorDescriptor {
                id: "gba-natsume-driver",
                semantic_version: 1,
                scope: "Two exact GBA Natsume driver profiles; song table, headers and visited sequence structure; no instrument-bank or MIDI conversion",
            },
            DetectorDescriptor {
                id: "engine-software-structure",
                semantic_version: 1,
                scope: "Engine Software-format 0x0121 banks, packed patterns, instruments and signed samples; XM playback is approximate",
            },
            DetectorDescriptor {
                id: "krawall-driver",
                semantic_version: 1,
                scope: "Recognized Krawall drivers, bank dereferences, bounded module graphs and original-driver startup",
            },
            DetectorDescriptor {
                id: "gax-native-driver",
                semantic_version: 1,
                scope: "Recognized GAX initialization, playback and interrupt routines with bounded song headers",
            },
            DetectorDescriptor {
                id: "musyx-driver",
                semantic_version: 2,
                scope: "Bounded MusyX song groups, native driver witnesses and original initialization handoff",
            },
            DetectorDescriptor {
                id: "aas-driver",
                semantic_version: 1,
                scope: "Two recognized Apex Audio System driver layouts, bound module tables and original initialization handoff",
            },
            DetectorDescriptor {
                id: "gba-descriptor-midi-driver",
                semantic_version: 1,
                scope: "Recognized GBA MIDI driver, sparse song descriptors, bounded SMF events and selected instrument banks",
            },
            DetectorDescriptor {
                id: "gba-nsq-driver",
                semantic_version: 1,
                scope: "Recognized NSQ native player with hashed filesystem, selector table and referenced NPF sample bank",
            },
            DetectorDescriptor {
                id: "gba-radriver",
                semantic_version: 1,
                scope: "Recognized RADriver layouts with native startup, effect banks and compressed music selectors",
            },
            DetectorDescriptor {
                id: "gba-gbass-driver",
                semantic_version: 1,
                scope: "Exact GBASS driver profiles with bounded song selectors and original-driver playback",
            },
            DetectorDescriptor {
                id: "gba-aas-stream-driver",
                semantic_version: 1,
                scope: "Exact Apex Audio System stream profiles with bound compressed music selectors and native playback",
            },
            DetectorDescriptor {
                id: "gba-aas-pcm-driver",
                semantic_version: 1,
                scope: "Exact Apex Audio System PCM profiles with bound sound-cue selectors and native playback",
            },
        ],
        System::Gb => &[
            MODULES,
            DetectorDescriptor {
                id: "gb-banked-driver",
                semantic_version: 2,
                scope: "Six exact banked GB driver profiles with bounded song tables and sequences",
            },
            DetectorDescriptor {
                id: "gb-native-driver",
                semantic_version: 2,
                scope: "Exact banked Game Boy drivers with qualified music selectors, closed driver state and DMG or CGB double-speed playback",
            },
            DetectorDescriptor {
                id: "gb-musyx-driver",
                semantic_version: 1,
                scope: "Exact original MusyX driver layouts with bounded songs, macro control flow and sample references; native CGB playback for the requested duration",
            },
            DetectorDescriptor {
                id: "gb-tose-driver",
                semantic_version: 1,
                scope: "Relocated classic TOSE driver with bounded four-channel music sequences and original DMG MBC1 playback",
            },
            DetectorDescriptor {
                id: "gb-quickthunder-driver",
                semantic_version: 1,
                scope: "Recognized QuickThunder drivers with bounded music structures and original Game Boy playback under the reported hardware profile",
            },
            DetectorDescriptor {
                id: "gb-driver-fingerprints",
                semantic_version: 1,
                scope: "Public sound-driver fingerprints with exact source offsets; possible families only, without song selection or playback qualification",
            },
            DetectorDescriptor {
                id: "gb-ghx-driver",
                semantic_version: 1,
                scope: "Qualified GHX routines, module/subsong selectors and bounded data closure with CGB double-speed playback",
            },
            DetectorDescriptor {
                id: "gb-sound-system-driver",
                semantic_version: 1,
                scope: "Recognized GB Sound System routines with bounded order/instrument data and qualified native hardware",
            },
            DetectorDescriptor {
                id: "gb-carillon-driver",
                semantic_version: 1,
                scope: "Qualified Carillon CGB interpreter, bounded orders/patterns/instruments, explicit alias collapse and original-driver playback",
            },
        ],
        System::Nes => &[
            MODULES,
            DetectorDescriptor {
                id: "nes-queue-driver",
                semantic_version: 2,
                scope: "NTSC NROM queue driver with matching native code, song data and cartridge mapping",
            },
            DetectorDescriptor {
                id: "nes-native-driver",
                semantic_version: 2,
                scope: "Exact NES driver profiles with qualified cartridge mapping, bounded native audio selectors and NTSC playback",
            },
            DetectorDescriptor {
                id: "nes-tose-driver",
                semantic_version: 1,
                scope: "Recognized NES TOSE routines with bounded selectors, qualified cartridge mapping and original NTSC playback",
            },
        ],
        System::Sms | System::Gg => &[
            MODULES,
            DetectorDescriptor {
                id: "sega-psg-driver",
                semantic_version: 1,
                scope: "Exact SMS/GG PSG driver profiles, bounded music selectors and original-driver playback with explicit NTSC timing",
            },
        ],
        System::Ws => &[
            MODULES,
            DetectorDescriptor {
                id: "ws-tose-driver",
                semantic_version: 2,
                scope: "Qualified TOSE-style eight-slot routines, bounded selectors and original WonderSwan playback",
            },
        ],
        _ => TRACKER,
    }
}

#[cfg(test)]
mod tests;
