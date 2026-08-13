#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MediaSlotId(pub String);

impl MediaSlotId {
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }
}

impl From<&str> for MediaSlotId {
    fn from(value: &str) -> Self {
        Self(value.to_owned())
    }
}

impl AsRef<str> for MediaSlotId {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MediaObjectId(pub String);

impl MediaObjectId {
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }
}

impl From<&str> for MediaObjectId {
    fn from(value: &str) -> Self {
        Self(value.to_owned())
    }
}

impl AsRef<str> for MediaObjectId {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum MediaEvent {
    Insert {
        slot: MediaSlotId,
        media_id: MediaObjectId,
        side: Option<u8>,
        write_protected: bool,
    },
    Eject {
        slot: MediaSlotId,
    },
    SelectSide {
        slot: MediaSlotId,
        side: u8,
    },
    SetWriteProtected {
        slot: MediaSlotId,
        write_protected: bool,
    },
}

impl MediaEvent {
    pub fn slot(&self) -> &MediaSlotId {
        match self {
            Self::Insert { slot, .. }
            | Self::Eject { slot }
            | Self::SelectSide { slot, .. }
            | Self::SetWriteProtected { slot, .. } => slot,
        }
    }
}

#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct TimedMediaEvent {
    pub frame: u64,
    pub sequence: u32,
    pub event: MediaEvent,
}

impl TimedMediaEvent {
    pub fn new(frame: u64, sequence: u32, event: MediaEvent) -> Self {
        Self {
            frame,
            sequence,
            event,
        }
    }
}

#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MediaSlotState {
    pub slot: MediaSlotId,
    pub media_id: Option<MediaObjectId>,
    pub side: Option<u8>,
    pub write_protected: bool,
    pub mutation_counter: u64,
}

impl MediaSlotState {
    pub fn empty(slot: impl Into<MediaSlotId>) -> Self {
        Self {
            slot: slot.into(),
            media_id: None,
            side: None,
            write_protected: false,
            mutation_counter: 0,
        }
    }

    pub fn inserted(&self) -> bool {
        self.media_id.is_some()
    }

    pub fn apply_event(&mut self, event: &MediaEvent) -> Result<(), MediaEventError> {
        if event.slot() != &self.slot {
            return Err(MediaEventError::WrongSlot {
                expected: self.slot.clone(),
                actual: event.slot().clone(),
            });
        }

        match event {
            MediaEvent::Insert {
                media_id,
                side,
                write_protected,
                ..
            } => {
                self.media_id = Some(media_id.clone());
                self.side = *side;
                self.write_protected = *write_protected;
                self.mutation_counter = 0;
            }
            MediaEvent::Eject { .. } => {
                self.media_id = None;
                self.side = None;
                self.write_protected = false;
                self.mutation_counter = 0;
            }
            MediaEvent::SelectSide { side, .. } => {
                if !self.inserted() {
                    return Err(MediaEventError::NoMediaInserted {
                        slot: self.slot.clone(),
                    });
                }
                self.side = Some(*side);
            }
            MediaEvent::SetWriteProtected {
                write_protected, ..
            } => {
                if !self.inserted() {
                    return Err(MediaEventError::NoMediaInserted {
                        slot: self.slot.clone(),
                    });
                }
                self.write_protected = *write_protected;
            }
        }

        Ok(())
    }

    pub fn record_mutation(&mut self) -> Result<(), MediaEventError> {
        if !self.inserted() {
            return Err(MediaEventError::NoMediaInserted {
                slot: self.slot.clone(),
            });
        }
        if self.write_protected {
            return Err(MediaEventError::WriteProtected {
                slot: self.slot.clone(),
            });
        }
        self.mutation_counter = self.mutation_counter.saturating_add(1);
        Ok(())
    }
}

#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MediaEventError {
    WrongSlot {
        expected: MediaSlotId,
        actual: MediaSlotId,
    },
    NoMediaInserted {
        slot: MediaSlotId,
    },
    WriteProtected {
        slot: MediaSlotId,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    fn slot() -> MediaSlotId {
        MediaSlotId::from("fds.drive0")
    }

    #[test]
    fn timed_media_events_sort_by_frame_then_sequence() {
        let mut events = [
            TimedMediaEvent::new(
                10,
                1,
                MediaEvent::Eject {
                    slot: MediaSlotId::from("b"),
                },
            ),
            TimedMediaEvent::new(
                5,
                0,
                MediaEvent::Eject {
                    slot: MediaSlotId::from("a"),
                },
            ),
            TimedMediaEvent::new(
                10,
                0,
                MediaEvent::Eject {
                    slot: MediaSlotId::from("c"),
                },
            ),
        ];

        events.sort();

        assert_eq!(events[0].frame, 5);
        assert_eq!(events[1].sequence, 0);
        assert_eq!(events[2].sequence, 1);
    }

    #[test]
    fn insert_and_select_side_update_slot_state() {
        let mut state = MediaSlotState::empty(slot());
        state
            .apply_event(&MediaEvent::Insert {
                slot: slot(),
                media_id: MediaObjectId::from("sha256:disk"),
                side: Some(0),
                write_protected: false,
            })
            .unwrap();

        assert!(state.inserted());
        assert_eq!(state.side, Some(0));

        state
            .apply_event(&MediaEvent::SelectSide {
                slot: slot(),
                side: 1,
            })
            .unwrap();
        assert_eq!(state.side, Some(1));
    }

    #[test]
    fn selecting_side_without_media_is_rejected() {
        let mut state = MediaSlotState::empty(slot());
        let err = state
            .apply_event(&MediaEvent::SelectSide {
                slot: slot(),
                side: 1,
            })
            .unwrap_err();

        assert!(matches!(err, MediaEventError::NoMediaInserted { .. }));
    }

    #[test]
    fn record_mutation_tracks_writes_and_rejects_write_protection() {
        let mut state = MediaSlotState::empty(slot());
        state
            .apply_event(&MediaEvent::Insert {
                slot: slot(),
                media_id: MediaObjectId::from("sha256:disk"),
                side: Some(0),
                write_protected: false,
            })
            .unwrap();

        state.record_mutation().unwrap();
        assert_eq!(state.mutation_counter, 1);

        state
            .apply_event(&MediaEvent::SetWriteProtected {
                slot: slot(),
                write_protected: true,
            })
            .unwrap();

        assert!(matches!(
            state.record_mutation(),
            Err(MediaEventError::WriteProtected { .. })
        ));
    }

    #[test]
    fn wrong_slot_event_is_rejected() {
        let mut state = MediaSlotState::empty(slot());
        let err = state
            .apply_event(&MediaEvent::Eject {
                slot: MediaSlotId::from("other"),
            })
            .unwrap_err();

        assert!(matches!(err, MediaEventError::WrongSlot { .. }));
    }
}
