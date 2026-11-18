use super::*;
use crate::hardware::vdp::owner::board::previous::{Key, TagSink, Tags};
use crate::hardware::vdp::owner::runtime::WorkWriteSink;

pub(super) trait Capture {
    type Tags: TagSink;
    fn offered(&mut self, board: &Board);
    fn completed(&mut self, receipt: main_bus::Receipt);
    fn before_write(&mut self, tags: &Self::Tags, half: u64, effect: (usize, u16, u8), work: &[u8]);
}

impl Capture for () {
    type Tags = ();
    fn offered(&mut self, _: &Board) {}
    fn completed(&mut self, _: main_bus::Receipt) {}
    fn before_write(&mut self, _: &(), _: u64, _: (usize, u16, u8), _: &[u8]) {}
}

#[derive(Default)]
pub(super) struct Previous {
    expected: Option<Key>,
    completed: bool,
    lanes: [Option<u8>; 2],
}

impl Previous {
    pub(super) fn finish(self, succeeded: bool) -> [Option<u8>; 2] {
        if succeeded && self.completed {
            self.lanes
        } else {
            [None, None]
        }
    }
}

impl Capture for Previous {
    type Tags = Tags;
    fn offered(&mut self, board: &Board) {
        self.completed = false;
        self.lanes = [None, None];
        self.expected = board
            .snapshot()
            .and_then(|snapshot| snapshot.pending)
            .map(|pending| Key {
                request: pending.request,
                start_half: pending.start_half,
            });
    }
    fn completed(&mut self, receipt: main_bus::Receipt) {
        self.completed = self.expected
            == Some(Key {
                request: receipt.request,
                start_half: receipt.start_half,
            });
    }
    fn before_write(&mut self, tags: &Tags, half: u64, effect: (usize, u16, u8), work: &[u8]) {
        let (ordinal, address, value) = effect;
        let Some(tag) = tags.0.get(ordinal).copied().flatten() else {
            return;
        };
        if Some(tag.key) != self.expected
            || tag.half != half
            || tag.ordinal != ordinal
            || tag.address != address
            || tag.value != value
        {
            return;
        }
        // Repeated physical strobes retain the first committed byte for each lane.
        if let Some(previous) = self.lanes.get_mut(tag.lane)
            && previous.is_none()
        {
            *previous = Some(work[usize::from(address)]);
        }
    }
}

pub(super) struct HalfCommit<'a, C: Capture, H> {
    pub physical: &'a mut H,
    pub half: u64,
    pub tags: C::Tags,
    pub capture: &'a mut C,
}

impl<C: Capture, H: HalfCapture> WorkWriteSink for HalfCommit<'_, C, H> {
    fn before_write(&mut self, ordinal: usize, address: u16, value: u8, work: &[u8]) {
        self.capture
            .before_write(&self.tags, self.half, (ordinal, address, value), work);
        self.physical.work_commit(ordinal, address, value, work);
    }
}

impl<S: PhysicalCapture> Context<'_, S> {
    pub(super) fn half_capturing<C: previous::Capture>(&mut self, capture: &mut C) -> Result<()> {
        let half = self
            .half_now()?
            .checked_add(1)
            .ok_or_else(|| anyhow::anyhow!("physical clock overflow"))?;
        self.validate_events()?;
        let result = (|| {
            let mut sound = self.state.sound.clone();
            let consumed = self.state.sound_event.is_some();
            if let Some(event) = self.state.sound_event {
                sound.apply_main_event(event)?;
            }
            let mut before = self.state.audio_pending.clone();
            before.extend(sound.take_audio_effects());
            let mut audio = None;
            events::apply_audio(self.audio, &mut audio, &before, half / 2)?;
            let boundary = boundary::Boundary {
                physical: self.physical.begin_half(half),
                previous: C::Tags::default(),
                board: self.state.board.clone(),
                #[cfg(test)]
                fifo_witness: self.fifo_witness.clone(),
                #[cfg(test)]
                work_witness: self.work_witness.clone(),
                sound,
                audio: audio.as_ref().unwrap_or(self.audio),
                read: protocol::ReadContext {
                    region: self.region,
                    input: self.input,
                    prefetch: *self.prefetch,
                },
            };
            let cartridge = &*self.cartridge;
            let prepared = self
                .vdp
                .prepare_owned_half(boundary, self.work, |address| {
                    cartridge
                        .read_sampled(address)
                        .ok_or_else(|| anyhow::anyhow!("unowned cartridge read {address:06x}"))
                })?;
            let (commit, boundary) = prepared.into_parts();
            let boundary::Boundary {
                previous,
                mut physical,
                board,
                mut sound,
                #[cfg(test)]
                fifo_witness,
                #[cfg(test)]
                mut work_witness,
                ..
            } = boundary;
            let effects = sound.take_audio_effects();
            let (ready, pending) = events::partition_audio(effects, half)?;
            events::apply_audio(self.audio, &mut audio, &ready, half / 2)?;
            let mut sink = previous::HalfCommit {
                physical: &mut physical,
                half,
                tags: previous,
                capture,
            };
            #[cfg(not(test))]
            let observation = if C::Tags::ENABLED || S::Half::ENABLED {
                commit.commit_capturing(&mut sink)
            } else {
                commit.commit()
            };
            #[cfg(test)]
            let observation = if let Some(witness) = &mut work_witness {
                commit.commit_with_work_witness(half, &mut witness.commits, &mut sink)
            } else if C::Tags::ENABLED || S::Half::ENABLED {
                commit.commit_capturing(&mut sink)
            } else {
                commit.commit()
            };
            self.state.board = board;
            #[cfg(test)]
            {
                self.fifo_witness = fifo_witness;
                self.work_witness = work_witness;
            }
            self.state.sound = sound;
            self.state.audio_pending = pending;
            if consumed {
                self.state.sound_event = None;
            }
            if let Some(audio) = audio {
                *self.audio = audio;
            }
            self.audio.request_advance_to(half / 2);
            *self.now = MasterTicks::new(half / 2);
            self.video.push_owned(observation.pixel);
            self.physical.accept_half(physical);
            self.physical.after_commit(self.state.sound.cpu());
            Ok(())
        })();
        if result.is_err() {
            self.physical.fault();
        }
        result
    }

    pub(super) fn transfer_capturing<C: previous::Capture>(
        &mut self,
        request: main_bus::Request,
        start: CpuClocks,
        capture: &mut C,
    ) -> Result<Transfer> {
        self.synchronize(start)?;
        let mut arbitration_begin = 0;
        let mut arbitration = 0_u32;
        while self.state.board.ready_tick().is_none() {
            if arbitration == 0 {
                arbitration_begin = self.half_now()?;
            }
            if arbitration == 300_000 {
                return self.grant_timeout(start, arbitration_begin, arbitration);
            }
            for _ in 0..14 {
                self.half()?;
            }
            arbitration += 1;
        }
        let address = request.access.bus_address;
        let target = if address & 0xffff00 == 0xa10000 {
            Some(ProtocolTarget::Io)
        } else if address & 0xff0000 == 0xa00000 {
            Some(ProtocolTarget::SoundRam)
        } else if address & 0xfffc00 == 0xa11000 {
            Some(ProtocolTarget::SoundControl)
        } else if address & 0xffff00 == 0xa13000 {
            Some(ProtocolTarget::Time)
        } else if address & 0xe700e0 == 0xc00000
            && (address >> 1) & 12 == 8
            && matches!(request.operation, main_bus::Operation::Write(_))
        {
            Some(ProtocolTarget::Psg)
        } else {
            None
        };
        if let Some(target) = target {
            self.state.board.offer_protocol(request, target)?;
        } else {
            self.state.board.offer(request)?;
        }
        capture.offered(&self.state.board);
        let begin = self.half_now()?;
        loop {
            ensure!(
                self.half_now()? - begin < 4_000_000,
                "MAIN transfer has no physical termination"
            );
            self.half_capturing(capture)?;
            if let Some(completed) = self.state.board.take_completed()? {
                if let Some(ProtocolCompletion::Write { binding, word }) = completed.protocol {
                    self.protocol_write(
                        binding.request.access,
                        binding.target,
                        word,
                        main_bus::ceil_tick(completed.main.start_half),
                        completed.main.end_tick(),
                    )?;
                }
                let wait_clocks = arbitration
                    .checked_add(completed.main.waits)
                    .ok_or_else(|| anyhow::anyhow!("MAIN wait overflow"))?;
                *self.clock = CpuClocks(
                    start
                        .0
                        .checked_add(4 + u64::from(wait_clocks))
                        .ok_or_else(|| anyhow::anyhow!("CPU clock overflow"))?,
                );
                self.state.transfers += 1;
                #[cfg(test)]
                if let Some(witness) = &mut self.work_witness {
                    witness.completion = Some(completed.main);
                }
                capture.completed(completed.main);
                return Ok(Transfer {
                    main: completed.main,
                    wait_clocks,
                    origin: completed.origin,
                });
            }
        }
    }

    pub(super) fn write_cycle_capturing<C: previous::Capture>(
        &mut self,
        access: BusAccess,
        value: u16,
        start: CpuClocks,
        capture: &mut C,
    ) -> CycleResult<(), BusError> {
        let value = match access.lanes {
            ByteLanes::Both => value,
            ByteLanes::Upper => u16::from_be_bytes([(value >> 8) as u8; 2]),
            ByteLanes::Lower => u16::from_be_bytes([value as u8; 2]),
        };
        let result = self.transfer_capturing(
            main_bus::Request {
                access,
                operation: main_bus::Operation::Write(value),
            },
            start,
            capture,
        );
        let waits = transfer_wait_clocks(&result);
        *self.prefetch = Prefetch::Empty;
        CycleResult {
            result: result.map(|_| ()).map_err(owned_error),
            wait_clocks: waits,
        }
    }
}
