use super::*;

mod host;

pub(crate) struct AudioTestDriver {
    consumer: rtrb::Consumer<QueuedAudioSample>,
    state: AudioPlaybackState,
}

impl AudioTestDriver {
    pub(crate) fn render(&mut self, samples: usize) -> Vec<f32> {
        let mut pcm = vec![0.0; samples];
        fill_output_f32(&mut pcm, 2, &mut self.consumer, &mut self.state);
        pcm
    }

    pub(crate) fn preroll(&self) -> usize {
        self.state.preroll_samples.load(Ordering::Relaxed)
    }

    pub(crate) fn buffered(&self) -> usize {
        self.consumer.slots()
    }

    pub(crate) fn underruns(&self) -> u64 {
        self.state.underruns.load(Ordering::Relaxed)
    }
}

impl AudioOutput {
    pub(crate) fn test_output(policy: AudioBufferPolicy) -> (Self, AudioTestDriver) {
        let rate = 48_000;
        let capacity = ring_buffer_capacity_for_policy(rate, policy);
        let (producer, consumer) = rtrb::RingBuffer::new(capacity);
        let preroll = Arc::new(AtomicUsize::new(playback_preroll_samples(
            rate,
            policy.normal_queue_ms(),
        )));
        let underruns = Arc::new(AtomicU64::new(0));
        let generation = Arc::new(AtomicU64::new(0));
        let state = AudioPlaybackState::new(
            Arc::clone(&preroll),
            Arc::clone(&underruns),
            Arc::clone(&generation),
        );
        let output = Self {
            _stream: None,
            queue: NativeSampleQueue::new(producer),
            source_sample_rate: rate,
            output_sample_rate: rate,
            capacity,
            low_pass_filter: OnePoleLowPass::default(),
            resampler: None,
            underruns,
            playback_preroll_samples: preroll,
            callback_samples: Arc::clone(&state.callback_samples),
            playback_generation: generation,
            playback_speed: 1,
            speedup_samples: Vec::new(),
            buffer_policy: policy,
            underrun_recovery: UnderrunRecovery::default(),
            underrun_clock: crate::platform::Instant::now(),
            buffer_fallback_pending: None,
            stream_errors: Arc::new(AtomicU64::new(0)),
        };
        (output, AudioTestDriver { consumer, state })
    }
}
