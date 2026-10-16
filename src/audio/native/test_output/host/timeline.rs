use super::*;

#[derive(Clone, Copy, serde::Serialize)]
pub(super) struct QueueRecord {
    pub at_ns: u64,
    pub fill_end_ns: u64,
    pub needed: usize,
    pub available_before: usize,
    pub available_after: usize,
    pub primed_before: bool,
    pub primed_after: bool,
    pub observed_generation_before: u64,
    pub observed_generation_after: u64,
    pub generation_before: u64,
    pub generation_after: u64,
    pub observed_preroll_before: usize,
    pub preroll_after: usize,
}

impl QueueRecord {
    pub(super) fn begin(
        epoch: Instant,
        now: Instant,
        needed: usize,
        consumer: &mut rtrb::Consumer<QueuedAudioSample>,
        state: &AudioPlaybackState,
    ) -> Self {
        Self {
            at_ns: now.duration_since(epoch).as_nanos() as u64,
            fill_end_ns: 0,
            needed,
            available_before: consumer.slots(),
            available_after: 0,
            primed_before: state.primed,
            primed_after: false,
            observed_generation_before: state.observed_generation,
            observed_generation_after: 0,
            generation_before: state.generation.load(Ordering::Acquire),
            generation_after: 0,
            observed_preroll_before: state.observed_preroll_samples,
            preroll_after: 0,
        }
    }

    pub(super) fn finish(
        &mut self,
        epoch: Instant,
        consumer: &mut rtrb::Consumer<QueuedAudioSample>,
        state: &AudioPlaybackState,
    ) {
        self.fill_end_ns = epoch.elapsed().as_nanos() as u64;
        self.available_after = consumer.slots();
        self.primed_after = state.primed;
        self.observed_generation_after = state.observed_generation;
        self.generation_after = state.generation.load(Ordering::Acquire);
        self.preroll_after = state.observed_preroll_samples;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn queue_records_separate_shortage_generation_and_preroll_transitions() {
        let epoch = Instant::now();
        let (mut output, mut driver) = AudioOutput::test_output(AudioBufferPolicy::Auto);
        for _ in 0..driver.preroll() {
            assert!(
                output
                    .queue
                    .producer
                    .push(QueuedAudioSample {
                        generation: 0,
                        value: 0.25
                    })
                    .is_ok()
            );
        }
        let mut pcm = [0.0; 960];
        fill_output_f32(&mut pcm, 2, &mut driver.consumer, &mut driver.state);
        assert!(driver.state.primed);
        while driver.consumer.slots() >= pcm.len() {
            fill_output_f32(&mut pcm, 2, &mut driver.consumer, &mut driver.state);
        }
        let mut shortage = QueueRecord::begin(
            epoch,
            Instant::now(),
            pcm.len(),
            &mut driver.consumer,
            &driver.state,
        );
        fill_output_f32(&mut pcm, 2, &mut driver.consumer, &mut driver.state);
        shortage.finish(epoch, &mut driver.consumer, &driver.state);
        assert!(shortage.primed_before && !shortage.primed_after);
        assert!(shortage.available_before < shortage.needed);
        assert_eq!(
            shortage.observed_generation_before,
            shortage.observed_generation_after
        );
        assert_eq!(shortage.observed_preroll_before, shortage.preroll_after);
        assert!(shortage.fill_end_ns >= shortage.at_ns);
        assert_eq!(driver.underruns(), 1);

        driver.state.primed = true;
        driver.state.generation.store(1, Ordering::Release);
        let mut changed = QueueRecord::begin(
            epoch,
            Instant::now(),
            pcm.len(),
            &mut driver.consumer,
            &driver.state,
        );
        fill_output_f32(&mut pcm, 2, &mut driver.consumer, &mut driver.state);
        changed.finish(epoch, &mut driver.consumer, &driver.state);
        assert!(changed.primed_before && !changed.primed_after);
        assert_eq!(changed.observed_generation_before, 0);
        assert_eq!(changed.observed_generation_after, 1);
        assert_eq!(driver.underruns(), 1);

        driver.state.primed = true;
        driver.state.preroll_samples.fetch_add(2, Ordering::Relaxed);
        let mut budget = QueueRecord::begin(
            epoch,
            Instant::now(),
            pcm.len(),
            &mut driver.consumer,
            &driver.state,
        );
        fill_output_f32(&mut pcm, 2, &mut driver.consumer, &mut driver.state);
        budget.finish(epoch, &mut driver.consumer, &driver.state);
        assert!(budget.primed_before && !budget.primed_after);
        assert!(budget.preroll_after > budget.observed_preroll_before);
        assert_eq!(driver.underruns(), 1);
    }
}
