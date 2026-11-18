use super::*;

pub(super) fn apply_audio(
    current: &Audio,
    staged: &mut Option<Audio>,
    effects: &[AudioEffect],
    end: u64,
) -> Result<()> {
    ensure!(
        current.state_now() <= end && !current.batch_active(),
        "invalid SOUND audio frontier"
    );
    if effects.is_empty() {
        return Ok(());
    }
    let candidate = staged.get_or_insert_with(|| current.clone());
    for effect in effects {
        ensure!(
            effect.tick() <= end,
            "SOUND audio effect precedes its integer boundary"
        );
        effect.apply(candidate)?;
    }
    Ok(())
}

pub(super) fn partition_audio(
    effects: Vec<AudioEffect>,
    half: u64,
) -> Result<(Vec<AudioEffect>, Vec<AudioEffect>)> {
    ensure!(effects.len() <= 8, "SOUND audio effect capacity");
    let mut ready = Vec::new();
    let mut pending = Vec::new();
    for effect in effects {
        ensure!(
            effect.tick() == half.div_ceil(2),
            "SOUND closing effect timestamp differs"
        );
        if effect.tick() <= half / 2 {
            ready.push(effect);
        } else {
            pending.push(effect);
        }
    }
    Ok((ready, pending))
}

impl<S: PhysicalCapture> Context<'_, S> {
    pub(super) fn validate_events(&self) -> Result<()> {
        let half = self.half_now()?;
        ensure!(
            self.audio.state_now() == half / 2 && !self.audio.batch_active(),
            "SOUND audio timeline differs"
        );
        ensure!(
            self.state.audio_pending.len() <= 8
                && self
                    .state
                    .audio_pending
                    .iter()
                    .all(|event| half & 1 != 0 && event.tick() == half.div_ceil(2)),
            "stale SOUND audio events"
        );
        if let Some(event) = self.state.sound_event {
            event.validate()?;
            ensure!(
                event.source_half == half && event.tick == half.div_ceil(2),
                "stale MAIN SOUND event"
            );
        }
        Ok(())
    }

    pub(super) fn write_sound_protocol(
        &mut self,
        access: BusAccess,
        target: ProtocolTarget,
        word: u16,
        tick: u64,
    ) -> Result<()> {
        self.validate_events()?;
        ensure!(
            self.state.sound_event.is_none(),
            "unretired MAIN SOUND event"
        );
        let half = self.half_now()?;
        let event = SoundEvent {
            request: main_bus::Request {
                access,
                operation: main_bus::Operation::Write(word),
            },
            target,
            source_half: half,
            tick,
        };
        event.validate()?;
        let mut sound = self.state.sound.clone();
        sound.apply_main_event(event)?;
        let effects = sound.take_audio_effects();
        let mut staged = None;
        apply_audio(self.audio, &mut staged, &effects, tick)?;
        self.state.sound_event = Some(event);
        Ok(())
    }
}
