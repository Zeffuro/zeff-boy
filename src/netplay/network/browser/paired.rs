use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;

use anyhow::{Result, ensure};

const CAPACITY: usize = 128;
pub(crate) const INPUT_CAPACITY: usize = CAPACITY - 8;
type Packet = (u8, Vec<u8>);

#[derive(Default)]
struct Channel {
    queued: VecDeque<Packet>,
    held: VecDeque<Packet>,
    hold_input: bool,
    closed: bool,
    refused: usize,
    high_water: usize,
}

pub(crate) struct Endpoint {
    incoming: Rc<RefCell<Channel>>,
    outgoing: Rc<RefCell<Channel>>,
}

pub(crate) struct Delivery(Rc<RefCell<Channel>>);

pub(crate) struct DeliveryState {
    pub held: usize,
    pub buffered: usize,
    pub refused: usize,
    pub high_water: usize,
    pub duplicate_latest: bool,
}

pub(crate) fn pair() -> (Endpoint, Endpoint, Delivery) {
    let a = Rc::new(RefCell::new(Channel::default()));
    let b = Rc::new(RefCell::new(Channel::default()));
    (
        Endpoint {
            incoming: a.clone(),
            outgoing: b.clone(),
        },
        Endpoint {
            incoming: b,
            outgoing: a.clone(),
        },
        Delivery(a),
    )
}

impl Delivery {
    pub(crate) fn state(&self) -> DeliveryState {
        let channel = self.0.borrow();
        DeliveryState {
            held: channel.held.len(),
            buffered: channel.queued.len() + channel.held.len(),
            refused: channel.refused,
            high_water: channel.high_water,
            duplicate_latest: channel.held.back().is_some_and(|packet| {
                channel
                    .held
                    .iter()
                    .rev()
                    .skip(1)
                    .any(|previous| previous == packet)
            }),
        }
    }
    pub(crate) fn hold_input(&self) {
        self.0.borrow_mut().hold_input = true;
    }
    pub(crate) fn release_input(&self) -> usize {
        let mut channel = self.0.borrow_mut();
        channel.hold_input = false;
        let count = channel.held.len();
        while let Some(packet) = channel.held.pop_front() {
            channel.queued.push_back(packet);
        }
        count
    }
}

impl Endpoint {
    pub(super) fn send(&self, kind: u8, packet: &[u8]) -> Result<bool> {
        let mut channel = self.outgoing.borrow_mut();
        ensure!(!channel.closed, "paired peer closed");
        ensure!(
            kind <= 1 && packet.len() <= 65_536,
            "paired packet exceeds bounds"
        );
        let buffered = channel.queued.len() + channel.held.len();
        if kind == 1 && buffered >= INPUT_CAPACITY {
            channel.refused = channel.refused.saturating_add(1);
            return Ok(false);
        }
        ensure!(buffered < CAPACITY, "paired transport capacity exceeded");
        channel.high_water = channel.high_water.max(buffered + 1);
        if kind == 1 && channel.hold_input {
            channel.held.push_back((kind, packet.to_vec()));
        } else {
            channel.queued.push_back((kind, packet.to_vec()));
        }
        Ok(true)
    }
    pub(super) fn receive(&self) -> Result<Option<Packet>> {
        let mut channel = self.incoming.borrow_mut();
        ensure!(!channel.closed, "paired endpoint closed");
        Ok(channel.queued.pop_front())
    }
    pub(super) fn close(&self) {
        self.incoming.borrow_mut().closed = true;
    }
}
