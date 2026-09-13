use super::catalog::{SongId, SongRef};
use super::formats::{AudioFormat, SongFormat};
use super::media::ScanInput;
use super::{SampleDirection, SampleInventory, ScanReport, SourceSpan};

pub(crate) fn song(
    input: &ScanInput,
    report: &ScanReport,
    id: SongId,
    format: SongFormat,
) -> String {
    let label = match report.song(id) {
        Some(SongRef::Mp2k(song)) => match song.table_entries.iter().map(|entry| entry.index).min()
        {
            Some(index) => {
                let ambiguous = report.candidates.iter().any(|other| {
                    other.header != song.header
                        && other.table_entries.iter().any(|entry| entry.index == index)
                });
                if ambiguous {
                    format!("{index:03} @{:08X}", song.header.effective_offset)
                } else {
                    format!("{index:03}")
                }
            }
            None => format!("Song @{:08X}", song.header.effective_offset),
        },
        Some(SongRef::Gax(song)) => {
            format!(
                "{} @{:08X}",
                component(&song.title, 60),
                song.header.effective_offset
            )
        }
        Some(SongRef::Gb(song)) => {
            format!("{:03} - {}", song.index, component(&song.title, 60))
        }
        Some(SongRef::Nes(song)) => {
            format!("{:03} - {}", song.index, component(&song.title, 60))
        }
        Some(SongRef::SegaPsg(song)) => format!("{:03} - {:02X}", song.index, song.raw_index),
        Some(SongRef::Natsume(song)) => format!("{:03}", song.index),
        Some(SongRef::EngineSoftware(song)) => format!("{:03}", song.index),
        Some(SongRef::Krawall(song)) => format!("{:03} - {:02}", song.index, song.subsong),
        Some(SongRef::GaxNative(song)) => format!("{:03}", song.index),
        Some(SongRef::Musyx(song)) => format!("{:03}", song.index),
        Some(SongRef::Aas(song)) => format!("{:03}", song.index),
        Some(SongRef::Gbass(song)) if song.native.module.is_some() => {
            let module = song.native.module.unwrap();
            format!(
                "Module {:02} - {:03} - {}",
                module.index,
                song.index,
                component(&song.title, 60)
            )
        }
        Some(SongRef::Gbass(song)) => match song.native.bank {
            Some(bank) => format!(
                "Bank {:02} - {:03} - {}",
                bank.index,
                song.index,
                component(&song.title, 60)
            ),
            None => format!("{:03}", song.index),
        },
        Some(SongRef::AasStream(song)) => format!("{:03}", song.index),
        Some(SongRef::AasPcm(song)) => format!("{:03}", song.index),
        Some(SongRef::NesNative(song)) => format!("{:02X}", song.raw_index),
        Some(SongRef::GbNative(song)) => format!("{:02X}", song.raw_index),
        Some(SongRef::DescriptorMidi(song)) => format!("{:03}", song.index),
        Some(SongRef::Nsq(song)) => format!("{:03}", song.index),
        Some(SongRef::Radriver(song)) => format!(
            "{} @{:08X}",
            component(&song.title, 60),
            song.header.effective_offset
        ),
        Some(SongRef::Vgm(log)) => component(&log.title, 60),
        Some(SongRef::Rip(_)) => String::new(),
        Some(SongRef::Module(song)) => match song.source {
            super::tracker::ModuleSource::Standalone { .. } => String::new(),
            super::tracker::ModuleSource::Embedded => {
                format!("{} @{:08X}", component(&song.name, 60), song.span.offset)
            }
        },
        Some(SongRef::Cdda(track)) => format!("Track {:02}", track.number),
        None => "Song".to_owned(),
    };
    let suffix = format.info().filename.strip_prefix("song").unwrap();
    filename(input, &label, suffix)
}

pub(crate) fn sample(input: &ScanInput, sample: &SampleInventory, format: AudioFormat) -> String {
    let direction = match sample.direction {
        SampleDirection::Forward => "",
        SampleDirection::Reverse => " reverse",
    };
    filename(
        input,
        &format!("Sample @{:08X}{direction}", sample.header.effective_offset),
        &format!(".{}", format.extension()),
    )
}

pub(crate) fn selection(input: &ScanInput, span: SourceSpan) -> String {
    filename(
        input,
        &format!("Bytes @{:08X}", span.effective_offset),
        ".zip",
    )
}

pub(crate) fn all_songs(input: &ScanInput, format: SongFormat) -> String {
    filename(
        input,
        &format!("All songs - {}", format.info().label),
        ".zip",
    )
}

pub(crate) fn report(input: &ScanInput) -> String {
    filename(input, "Audio discovery", ".json")
}

fn filename(input: &ScanInput, label: &str, suffix: &str) -> String {
    let source = source_name(input);
    if label.is_empty() {
        format!("{source}{suffix}")
    } else {
        format!("{source} - {}{suffix}", component(label, 90))
    }
}

fn source_name(input: &ScanInput) -> String {
    if let Some(name) = &input.display_name
        && !name.trim().is_empty()
    {
        return component(name, 90);
    }
    if let Some(member) = input
        .provenance
        .as_ref()
        .and_then(|source| source.source.selected_member.as_ref())
    {
        let name = member
            .name
            .rsplit(['/', '\\'])
            .next()
            .unwrap_or(&member.name);
        let stem = name.rsplit_once('.').map_or(name, |(stem, _)| stem);
        return component(stem, 90);
    }
    "Audio".to_owned()
}

fn component(value: &str, max_bytes: usize) -> String {
    let mut output = String::new();
    for ch in value.trim().chars() {
        let ch = if ch.is_control()
            || matches!(ch, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|')
        {
            '_'
        } else if ch.is_whitespace() {
            ' '
        } else {
            ch
        };
        if ch == ' ' && output.ends_with(' ') {
            continue;
        }
        if output.len() + ch.len_utf8() > max_bytes {
            break;
        }
        output.push(ch);
    }
    let output = output.trim_matches([' ', '.']);
    if output.is_empty() {
        "Audio".to_owned()
    } else {
        let stem = output.split('.').next().unwrap().to_ascii_uppercase();
        let reserved = matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
            || stem
                .strip_prefix("COM")
                .or_else(|| stem.strip_prefix("LPT"))
                .is_some_and(|number| {
                    matches!(
                        number,
                        "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³"
                    )
                });
        if reserved {
            format!("_{output}")
        } else {
            output.to_owned()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::SongTableReference;
    use super::*;
    use std::sync::atomic::AtomicBool;
    use zeff_emu_common::system::System;

    fn input() -> ScanInput {
        ScanInput {
            cdda: None,
            system: Some(System::Gba),
            standalone_audio: None,
            bytes: super::super::test_support::gba_fixture().into(),
            provenance: None,
            analysis_profile: "naming-test",
            display_name: Some("Fixture Game".to_owned()),
        }
    }

    #[test]
    fn module_gbass_names_distinguish_repeated_raw_selectors() {
        let mut input = input();
        input.bytes = zeff_audio_discovery::gbass::fixture_rom_module().into();
        let report = input
            .analyze(Default::default(), &AtomicBool::new(false))
            .scan;
        let names: std::collections::BTreeSet<_> = (0..4)
            .map(|index| song(&input, &report, SongId::Gbass(index), SongFormat::Gsf))
            .collect();
        assert_eq!(names.len(), 4);
        for module in ["Module 00", "Module 01"] {
            assert_eq!(names.iter().filter(|name| name.contains(module)).count(), 2);
        }
    }

    #[test]
    fn banked_gbass_names_distinguish_raw_selectors_and_keep_old_names() {
        let mut input = input();
        input.bytes = zeff_audio_discovery::gbass::fixture_rom_banked().into();
        let report = input
            .analyze(Default::default(), &AtomicBool::new(false))
            .scan;
        let names: std::collections::BTreeSet<_> = (0..4)
            .map(|index| song(&input, &report, SongId::Gbass(index), SongFormat::Gsf))
            .collect();
        assert_eq!(names.len(), 4);
        assert_eq!(
            names.iter().filter(|name| name.contains("Bank 00")).count(),
            2
        );
        assert_eq!(
            names.iter().filter(|name| name.contains("Bank 01")).count(),
            2
        );
        input.bytes = zeff_audio_discovery::gbass::fixture_rom().into();
        let report = input
            .analyze(Default::default(), &AtomicBool::new(false))
            .scan;
        assert_eq!(
            song(&input, &report, SongId::Gbass(0), SongFormat::Gsf),
            "Fixture Game - 000.gsf"
        );
    }

    #[test]
    fn verified_numbers_keep_aliases_stable_and_disambiguate_multiple_tables() {
        let input = input();
        let mut report = input
            .analyze(Default::default(), &AtomicBool::new(false))
            .scan;
        assert_eq!(
            song(&input, &report, SongId::Mp2k(0), SongFormat::Midi),
            "Fixture Game - Song @00000100.mid"
        );
        let entry = |index| SongTableReference {
            table_offset: 0x800,
            index,
            entry: crate::audio_discovery::test_support::rom_span(0x800 + index as usize * 8, 8),
            player: 0,
        };
        report.candidates[0].table_entries = vec![entry(20), entry(12)];
        assert_eq!(
            song(
                &input,
                &report,
                SongId::Mp2k(0),
                SongFormat::Audio(AudioFormat::Wav)
            ),
            "Fixture Game - 012.wav"
        );
        assert_eq!(
            song(&input, &report, SongId::Mp2k(0), SongFormat::MidiSoundFont),
            "Fixture Game - 012-midi-sf2.zip"
        );
        let mut other = report.candidates[0].clone();
        other.header = crate::audio_discovery::test_support::rom_span(0x900, 16);
        report.candidates.push(other);
        assert_eq!(
            song(&input, &report, SongId::Mp2k(0), SongFormat::Midi),
            "Fixture Game - 012 @00000100.mid"
        );
        assert_eq!(
            song(&input, &report, SongId::Mp2k(1), SongFormat::Midi),
            "Fixture Game - 012 @00000900.mid"
        );
    }

    #[test]
    fn names_stay_single_bounded_unicode_filename_components() {
        let mut input = input();
        input.display_name = Some(format!("../CON:folder\\\"bad?{} ", "音".repeat(200)));
        let name = report(&input);
        assert!(name.len() <= 200);
        assert!(!name.starts_with('.'));
        assert!(
            !name
                .chars()
                .any(|ch| ch.is_control() || "/\\:*?\"<>|".contains(ch))
        );
        assert!(name.contains('音'));
        assert!(name.ends_with(" - Audio discovery.json"));
        assert_eq!(component("CON", 90), "_CON");
        assert_eq!(component("lpt1.notes", 90), "_lpt1.notes");
    }

    #[test]
    fn forward_and_reverse_sample_defaults_are_distinct() {
        let input = input();
        let report = input.analyze(Default::default(), &AtomicBool::new(false));
        let mut selected = *super::super::assets::samples(&report.scan.candidates[0])
            .next()
            .unwrap();
        assert_eq!(
            sample(&input, &selected, AudioFormat::Flac),
            "Fixture Game - Sample @00000300.flac"
        );
        selected.direction = SampleDirection::Reverse;
        assert_eq!(
            sample(&input, &selected, AudioFormat::Flac),
            "Fixture Game - Sample @00000300 reverse.flac"
        );
    }
}
