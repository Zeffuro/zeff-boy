use super::*;

impl ExplicitExportSettings {
    fn timing(&self) -> bool {
        self.loops || self.max_seconds
    }

    fn mp2k_controls(&self) -> bool {
        self.midi_channel10 || self.bank_select || self.gain
    }

    fn mp2k_settings(&self) -> bool {
        self.fade_seconds || self.mp2k_controls()
    }
}

impl OfflineExport {
    pub(super) fn validate_settings(&self, id: SongId) -> anyhow::Result<()> {
        if self.format.is_gsf() && !matches!(id, SongId::Mp2k(_)) {
            ensure!(
                !self.explicit.loops
                    && !self.explicit.sample_rate
                    && !self.explicit.mp2k_controls(),
                "native GSF accepts duration and fade tags; loop, sample-rate and MIDI settings do not apply"
            );
            return Ok(());
        }
        if recording_player(id, self.format) {
            ensure!(
                !self.explicit.loops,
                "--audio-loops does not apply to duration-based audio recording"
            );
            ensure!(
                !self.explicit.mp2k_controls(),
                "MIDI channel-10, bank-select and gain settings do not apply to this audio player"
            );
            ensure!(
                !(self.explicit.sample_rate
                    || self.explicit.max_seconds
                    || self.explicit.fade_seconds)
                    || matches!(self.format, SongFormat::Audio(_)),
                "recording duration, fade and sample-rate settings require --audio-export WAV, FLAC, or Ogg"
            );
            return Ok(());
        }
        ensure!(
            !self.explicit.sample_rate || matches!(id, SongId::Mp2k(_)),
            "--audio-sample-rate only applies to MP2k WAV, FLAC, or Ogg exports"
        );
        ensure!(
            !self.explicit.sample_rate || matches!(self.format, SongFormat::Audio(_)),
            "--audio-sample-rate requires --audio-export WAV, FLAC, or Ogg"
        );
        ensure!(
            !self.explicit.mp2k_settings() || matches!(id, SongId::Mp2k(_)),
            "fade, MIDI channel-10, bank-select and gain settings only apply to MP2k exports"
        );
        ensure!(
            !self.explicit.timing()
                || matches!(id, SongId::Mp2k(_))
                || (matches!(id, SongId::Gb(_) | SongId::Nes(_))
                    && self.format == SongFormat::Midi),
            "loop and duration settings require MP2k export or Game Boy/NES MIDI; preserved source assets retain their original data"
        );
        Ok(())
    }

    pub(super) fn options_for(&self, id: SongId) -> anyhow::Result<RenderOptions> {
        self.validate_settings(id)?;
        let mut options = self.options;
        if (recording_player(id, self.format) && matches!(self.format, SongFormat::Audio(_)))
            || (self.format.is_gsf() && !matches!(id, SongId::Mp2k(_)))
        {
            if !self.explicit.max_seconds {
                options.max_seconds = DEFAULT_DURATION_SECONDS;
            }
            ensure!(
                u16::from(options.fade_seconds) <= options.max_seconds,
                "fade duration must not exceed the recording duration"
            );
        }
        Ok(options)
    }
}

pub(super) fn recording_player(id: SongId, format: SongFormat) -> bool {
    matches!(
        id,
        SongId::Natsume(_)
            | SongId::EngineSoftware(_)
            | SongId::Gax(_)
            | SongId::Krawall(_)
            | SongId::GaxNative(_)
            | SongId::Musyx(_)
            | SongId::Aas(_)
            | SongId::DescriptorMidi(_)
            | SongId::Nsq(_)
            | SongId::Radriver(_)
            | SongId::Gbass(_)
            | SongId::AasStream(_)
            | SongId::AasPcm(_)
            | SongId::NesNative(_)
            | SongId::GbNative(_)
            | SongId::GbMusyx(_)
            | SongId::GbTose(_)
            | SongId::GbGhx(_)
            | SongId::GbSoundSystem(_)
            | SongId::GbCarillon(_)
            | SongId::WsTose(_)
            | SongId::GbQuickThunder(_)
            | SongId::NesTose(_)
            | SongId::SegaPsg(_)
    ) || (matches!(id, SongId::Gb(_) | SongId::Nes(_)) && matches!(format, SongFormat::Audio(_)))
}
