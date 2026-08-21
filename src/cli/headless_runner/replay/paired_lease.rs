use crate::emu_backend::EmuBackend;
use crate::link::{LinkSessionError, LinkTransport, RemoteLink};
use std::fmt;
use zeff_emu_common::replay::ReplayGameBoyLinkAction;
use zeff_gb_core::emulator::{FrameSliceCursor, FrameSliceOutcome};

#[derive(Default)]
pub(super) struct PairedGameBoyFrameLease {
    cursor: Option<FrameSliceCursor>,
    activation_tick: Option<u64>,
    activation_consumed: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum PairedGameBoyFrameLeaseOutcome {
    Boundary,
    FrameComplete,
    Suspended,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum PairedGameBoyPointRelation {
    NotRequested,
    Before,
    Exact,
    Overshot,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct PairedGameBoyDirectStep {
    pub outcome: PairedGameBoyFrameLeaseOutcome,
    pub boundary_reached: bool,
    pub point: PairedGameBoyPointRelation,
    pub queued_master_action: Option<ReplayGameBoyLinkAction>,
}

#[derive(Debug)]
pub(super) enum PairedGameBoyFrameLeaseError {
    Link(LinkSessionError),
    ActivationRequiresLink {
        activation_tick: u64,
    },
    ActivationNotReached {
        activation_tick: u64,
        current_tick: u64,
    },
}

impl fmt::Display for PairedGameBoyFrameLeaseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Link(error) => write!(f, "link error: {error:?}"),
            Self::ActivationRequiresLink { activation_tick } => {
                write!(
                    f,
                    "activation tick {activation_tick} requires a Game Boy link"
                )
            }
            Self::ActivationNotReached {
                activation_tick,
                current_tick,
            } => write!(
                f,
                "frame completed at tick {current_tick} before activation tick {activation_tick}"
            ),
        }
    }
}

impl std::error::Error for PairedGameBoyFrameLeaseError {}

impl PairedGameBoyFrameLease {
    pub(super) fn needs_frame_setup(&self) -> bool {
        self.cursor.is_none()
    }

    pub(super) fn begin(
        &mut self,
        backend: &EmuBackend,
        activation_tick: Option<u64>,
    ) -> Result<(), LinkSessionError> {
        assert!(
            self.cursor.is_none(),
            "replay frame lease is already active"
        );
        self.cursor = Some(backend.begin_game_boy_frame_slice()?);
        self.activation_tick = activation_tick;
        self.activation_consumed = false;
        Ok(())
    }

    pub(super) fn step<T: LinkTransport>(
        &mut self,
        backend: &mut EmuBackend,
        link: Option<&mut RemoteLink<T>>,
    ) -> Result<PairedGameBoyFrameLeaseOutcome, PairedGameBoyFrameLeaseError> {
        if link.is_none()
            && let Some(activation_tick) = self.activation_tick
        {
            return Err(PairedGameBoyFrameLeaseError::ActivationRequiresLink { activation_tick });
        }
        if link.is_none() {
            backend.set_link_peer_present(false);
            return self
                .step_direct_until(backend, None, false)
                .map(|progress| progress.outcome);
        }
        let cursor = self
            .cursor
            .as_mut()
            .expect("replay frame lease is not active");
        let activation_tick = (!self.activation_consumed)
            .then_some(self.activation_tick)
            .flatten();
        let outcome = match link {
            Some(RemoteLink::GameBoy(link)) => backend
                .step_game_boy_frame_slice_with_remote_link(link, cursor, activation_tick)
                .map_err(Self::link_error)?,
            Some(RemoteLink::WonderSwan(_)) => {
                return Err(Self::link_error(LinkSessionError::IncompatibleSystems));
            }
            None => unreachable!("no-link frame slice returned above"),
        };
        let current_tick = backend
            .game_boy_cpu_cycles()
            .expect("paired Game Boy frame lease requires a GB backend");
        if activation_tick.is_some_and(|tick| current_tick >= tick) {
            self.activation_consumed = true;
        }
        if matches!(outcome, FrameSliceOutcome::FrameComplete)
            && let Some(activation_tick) = activation_tick
            && !self.activation_consumed
        {
            return Err(PairedGameBoyFrameLeaseError::ActivationNotReached {
                activation_tick,
                current_tick,
            });
        }
        Ok(match outcome {
            FrameSliceOutcome::Boundary => PairedGameBoyFrameLeaseOutcome::Boundary,
            FrameSliceOutcome::FrameComplete => PairedGameBoyFrameLeaseOutcome::FrameComplete,
            FrameSliceOutcome::Suspended => PairedGameBoyFrameLeaseOutcome::Suspended,
        })
    }

    pub(super) fn step_direct_until(
        &mut self,
        backend: &mut EmuBackend,
        target_tick: Option<u64>,
        stop_on_link_action: bool,
    ) -> Result<PairedGameBoyDirectStep, PairedGameBoyFrameLeaseError> {
        let cursor = self
            .cursor
            .as_mut()
            .expect("replay frame lease is not active");
        let progress = backend
            .step_game_boy_frame_slice_until(cursor, target_tick, stop_on_link_action)
            .map_err(Self::link_error)?;
        let current_tick = backend
            .game_boy_cpu_cycles()
            .expect("paired Game Boy frame lease requires a GB backend");
        let point = match target_tick {
            None => PairedGameBoyPointRelation::NotRequested,
            Some(target) if current_tick < target => PairedGameBoyPointRelation::Before,
            Some(target) if current_tick == target => PairedGameBoyPointRelation::Exact,
            Some(_) => PairedGameBoyPointRelation::Overshot,
        };
        Ok(PairedGameBoyDirectStep {
            outcome: match progress.outcome {
                FrameSliceOutcome::Boundary => PairedGameBoyFrameLeaseOutcome::Boundary,
                FrameSliceOutcome::FrameComplete => PairedGameBoyFrameLeaseOutcome::FrameComplete,
                FrameSliceOutcome::Suspended => PairedGameBoyFrameLeaseOutcome::Suspended,
            },
            boundary_reached: progress.boundary_reached,
            point,
            queued_master_action: backend
                .game_boy_link_replay_state()
                .and_then(|state| state.queued_master_action),
        })
    }

    pub(super) fn commit_frame(&mut self) {
        assert!(
            self.cursor.is_some(),
            "replay frame lease must be active before committing"
        );
        self.cursor = None;
        self.activation_tick = None;
        self.activation_consumed = false;
    }

    fn link_error(error: LinkSessionError) -> PairedGameBoyFrameLeaseError {
        PairedGameBoyFrameLeaseError::Link(error)
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use zeff_gb_core::hardware::types::constants::{SERIAL_SB, SERIAL_SC};
    use zeff_gb_core::hardware::types::hardware_mode::HardwareModePreference;

    use super::*;

    fn test_backend() -> EmuBackend {
        let rom = vec![0u8; 0x8000];
        let emulator =
            zeff_gb_core::emulator::Emulator::from_rom_data(&rom, HardwareModePreference::Auto)
                .expect("GB emulator should initialize");
        EmuBackend::from_gb(emulator, PathBuf::from("test.gb"))
    }

    #[test]
    fn completed_frame_requires_owner_commit_before_next_frame() {
        let mut backend = test_backend();
        let mut lease = PairedGameBoyFrameLease::default();
        let frame_before = backend.frame_count();

        lease.begin(&backend, None).unwrap();
        assert!(matches!(
            lease.step::<crate::link::transport::LocalLinkTransport>(&mut backend, None),
            Ok(PairedGameBoyFrameLeaseOutcome::FrameComplete)
        ));
        assert_eq!(backend.frame_count(), frame_before + 1);
        assert!(matches!(
            lease.step::<crate::link::transport::LocalLinkTransport>(&mut backend, None),
            Ok(PairedGameBoyFrameLeaseOutcome::FrameComplete)
        ));
        assert_eq!(backend.frame_count(), frame_before + 1);

        lease.commit_frame();
        assert!(lease.needs_frame_setup());
        lease.begin(&backend, None).unwrap();
        assert!(matches!(
            lease.step::<crate::link::transport::LocalLinkTransport>(&mut backend, None),
            Ok(PairedGameBoyFrameLeaseOutcome::FrameComplete)
        ));
        assert_eq!(backend.frame_count(), frame_before + 2);
    }

    #[test]
    fn resumes_boundary_with_the_same_cursor_until_frame_complete() {
        let mut left = test_backend();
        let mut right = test_backend();
        let (left_transport, right_transport) = crate::link::transport::LocalLinkTransport::pair();
        let mut left_link = RemoteLink::GameBoy(crate::link::gb::GameBoyRemoteLink::new(
            crate::link::LinkSession::new(
                left_transport,
                crate::link::LinkSystemType::GameBoy,
                crate::link::LinkEndpointId(1),
            ),
        ));
        let mut right_link =
            crate::link::gb::GameBoyRemoteLink::new(crate::link::LinkSession::new(
                right_transport,
                crate::link::LinkSystemType::GameBoy,
                crate::link::LinkEndpointId(2),
            ));
        let EmuBackend::Gb(left_backend) = &mut left else {
            unreachable!();
        };
        left_backend.emu.write_byte(SERIAL_SB, 0xAB);
        left_backend.emu.write_byte(SERIAL_SC, 0x81);
        let EmuBackend::Gb(right_backend) = &mut right else {
            unreachable!();
        };
        right_backend.emu.write_byte(SERIAL_SB, 0x34);
        right_backend.emu.write_byte(SERIAL_SC, 0x80);

        let mut lease = PairedGameBoyFrameLease::default();
        let frame_before = left.frame_count();
        lease.begin(&left, left.game_boy_cpu_cycles()).unwrap();
        assert!(matches!(
            lease.step(&mut left, Some(&mut left_link)),
            Ok(PairedGameBoyFrameLeaseOutcome::Boundary)
        ));
        assert_eq!(left.frame_count(), frame_before);

        right_link.poll_backend(&mut right).unwrap();
        assert!(matches!(
            lease.step(&mut left, Some(&mut left_link)),
            Ok(PairedGameBoyFrameLeaseOutcome::FrameComplete)
        ));
        assert_eq!(left.frame_count(), frame_before + 1);
        assert!(matches!(
            lease.step(&mut left, Some(&mut left_link)),
            Ok(PairedGameBoyFrameLeaseOutcome::FrameComplete)
        ));
        assert_eq!(left.frame_count(), frame_before + 1);
    }

    #[test]
    fn activation_tick_requires_a_game_boy_link() {
        let mut backend = test_backend();
        let mut lease = PairedGameBoyFrameLease::default();
        let activation_tick = backend.game_boy_cpu_cycles().unwrap() + 1_000_000;
        lease.begin(&backend, Some(activation_tick)).unwrap();

        assert!(matches!(
            lease.step::<crate::link::transport::LocalLinkTransport>(&mut backend, None),
            Err(PairedGameBoyFrameLeaseError::ActivationRequiresLink {
                activation_tick: actual,
            }) if actual == activation_tick
        ));
    }

    #[test]
    fn applies_the_due_replay_link_state_at_activation_tick() {
        let mut backend = test_backend();
        let (transport, _peer_transport) = crate::link::transport::LocalLinkTransport::pair();
        let state = zeff_emu_common::replay::ReplayGameBoyLinkState {
            peer_present: true,
            pending_master_byte: None,
            pending_master_response: None,
            pending_master_completion_ready: false,
            queued_master_action: None,
            pending_passive_completion: None,
            serial_generation: 7,
        };
        let activation_tick = backend.game_boy_cpu_cycles().unwrap() + 4;
        let mut link =
            RemoteLink::GameBoy(crate::link::gb::GameBoyRemoteLink::with_replay_schedules(
                crate::link::LinkSession::new(
                    transport,
                    crate::link::LinkSystemType::GameBoy,
                    crate::link::LinkEndpointId(1),
                ),
                Vec::new(),
                vec![(backend.frame_count(), activation_tick, state)],
            ));
        let mut lease = PairedGameBoyFrameLease::default();
        lease.begin(&backend, Some(activation_tick)).unwrap();

        assert!(matches!(
            lease.step(&mut backend, Some(&mut link)),
            Ok(PairedGameBoyFrameLeaseOutcome::FrameComplete)
        ));
        assert_eq!(backend.game_boy_link_replay_state(), Some(state));
    }

    #[test]
    fn direct_step_reports_exact_and_overshot_points() {
        let mut exact_backend = test_backend();
        let exact_target = exact_backend.game_boy_cpu_cycles().unwrap() + 4;
        let mut exact_lease = PairedGameBoyFrameLease::default();
        exact_lease.begin(&exact_backend, None).unwrap();
        let exact = exact_lease
            .step_direct_until(&mut exact_backend, Some(exact_target), false)
            .unwrap();
        assert_eq!(exact.outcome, PairedGameBoyFrameLeaseOutcome::Boundary);
        assert!(exact.boundary_reached);
        assert_eq!(exact.point, PairedGameBoyPointRelation::Exact);

        let mut overshot_backend = test_backend();
        let overshot_target = overshot_backend.game_boy_cpu_cycles().unwrap() + 1;
        let mut overshot_lease = PairedGameBoyFrameLease::default();
        overshot_lease.begin(&overshot_backend, None).unwrap();
        let overshot = overshot_lease
            .step_direct_until(&mut overshot_backend, Some(overshot_target), false)
            .unwrap();
        assert_eq!(overshot.outcome, PairedGameBoyFrameLeaseOutcome::Boundary);
        assert!(overshot.boundary_reached);
        assert_eq!(overshot.point, PairedGameBoyPointRelation::Overshot);
    }

    #[test]
    fn frame_completion_retains_a_same_instruction_point_before_commit() {
        let mut probe = test_backend();
        let mut probe_cursor = probe.begin_game_boy_frame_slice().unwrap();
        assert_eq!(
            probe
                .step_game_boy_frame_slice_until(&mut probe_cursor, None, false)
                .unwrap()
                .outcome,
            FrameSliceOutcome::FrameComplete
        );
        let completion_tick = probe.game_boy_cpu_cycles().unwrap();

        let mut backend = test_backend();
        let frame_before = backend.frame_count();
        let mut lease = PairedGameBoyFrameLease::default();
        lease.begin(&backend, None).unwrap();
        let progress = lease
            .step_direct_until(&mut backend, Some(completion_tick), false)
            .unwrap();

        assert_eq!(
            progress.outcome,
            PairedGameBoyFrameLeaseOutcome::FrameComplete
        );
        assert!(progress.boundary_reached);
        assert_eq!(progress.point, PairedGameBoyPointRelation::Exact);
        assert_eq!(backend.frame_count(), frame_before + 1);
        assert!(!lease.needs_frame_setup());

        lease.commit_frame();
        assert!(lease.needs_frame_setup());
    }

    #[test]
    fn direct_step_returns_an_existing_action_without_advancing() {
        let mut backend = test_backend();
        backend.set_link_peer_present(true);
        let EmuBackend::Gb(gb) = &mut backend else {
            unreachable!();
        };
        gb.emu.write_byte(SERIAL_SB, 0xA5);
        gb.emu.write_byte(SERIAL_SC, 0x81);
        let tick_before = gb.emu.cpu_cycles();
        let state_before = gb.emu.game_boy_link_replay_state();

        let mut lease = PairedGameBoyFrameLease::default();
        lease.begin(&backend, None).unwrap();
        let progress = lease.step_direct_until(&mut backend, None, true).unwrap();

        assert_eq!(progress.outcome, PairedGameBoyFrameLeaseOutcome::Boundary);
        assert!(progress.boundary_reached);
        assert_eq!(progress.point, PairedGameBoyPointRelation::NotRequested);
        assert_eq!(
            progress.queued_master_action,
            state_before.queued_master_action
        );
        assert_eq!(backend.game_boy_cpu_cycles(), Some(tick_before));
        assert_eq!(backend.game_boy_link_replay_state(), Some(state_before));
    }
}
