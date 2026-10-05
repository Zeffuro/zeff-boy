use std::sync::atomic::Ordering;

use super::*;

#[derive(Debug)]
pub struct ConnectionTerminated;

impl std::fmt::Display for ConnectionTerminated {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("connection is terminal")
    }
}

impl std::error::Error for ConnectionTerminated {}

struct State {
    terminal: Arc<AtomicBool>,
    sent_close: AtomicBool,
    received_close: AtomicBool,
}

pub struct Sender {
    connection: Connection,
    state: Arc<State>,
}

pub struct Receiver {
    connection: Connection,
    state: Arc<State>,
}

impl Connection {
    pub fn split(mut self) -> Result<(Sender, Receiver)> {
        ensure!(
            !self.terminal && !self.sent_close && !self.received_close,
            "only an open connection can split"
        );
        let terminal = self
            .cancellation
            .clone()
            .unwrap_or_else(|| Arc::new(AtomicBool::new(false)));
        self.cancellation = Some(Arc::clone(&terminal));
        let stream = self
            .stream
            .try_clone()
            .context("cloning authenticated receive socket")?;
        let receiver = Connection {
            io: Driver::new(&stream)?,
            stream,
            player: self.player,
            secret: self.secret,
            transcript: self.transcript,
            send_sequence: self.send_sequence,
            receive_sequence: self.receive_sequence,
            sent_close: false,
            received_close: false,
            terminal: false,
            cancellation: self.cancellation.clone(),
        };
        let state = Arc::new(State {
            terminal,
            sent_close: AtomicBool::new(false),
            received_close: AtomicBool::new(false),
        });
        Ok((
            Sender {
                connection: self,
                state: Arc::clone(&state),
            },
            Receiver {
                connection: receiver,
                state,
            },
        ))
    }
}

impl Sender {
    pub fn send(&mut self, message: &Message) -> Result<()> {
        ensure!(
            !self.state.terminal.load(Ordering::Acquire),
            ConnectionTerminated
        );
        self.connection.received_close = self.state.received_close.load(Ordering::Acquire);
        let result = self.connection.send(message);
        if result.is_ok() && matches!(message, Message::Close { .. }) {
            self.state.sent_close.store(true, Ordering::Release);
        }
        if result.is_err()
            || (self.state.sent_close.load(Ordering::Acquire)
                && self.state.received_close.load(Ordering::Acquire))
        {
            self.terminate();
        }
        result
    }

    fn terminate(&mut self) {
        self.state.terminal.store(true, Ordering::Release);
        self.connection.terminate();
    }
}

impl Receiver {
    pub fn receive(&mut self) -> Result<Message> {
        ensure!(
            !self.state.terminal.load(Ordering::Acquire),
            ConnectionTerminated
        );
        self.connection.sent_close = self.state.sent_close.load(Ordering::Acquire);
        let result = self.connection.receive().and_then(|message| {
            ensure!(
                !self.state.sent_close.load(Ordering::Acquire)
                    || matches!(message, Message::Close { .. }),
                "gameplay packet after local Close"
            );
            Ok(message)
        });
        if matches!(result, Ok(Message::Close { .. })) {
            self.state.received_close.store(true, Ordering::Release);
        }
        if result.is_err()
            || (self.state.sent_close.load(Ordering::Acquire)
                && self.state.received_close.load(Ordering::Acquire))
        {
            self.terminate();
        }
        result
    }

    fn terminate(&mut self) {
        self.state.terminal.store(true, Ordering::Release);
        self.connection.terminate();
    }
}

impl Drop for Sender {
    fn drop(&mut self) {
        self.terminate();
    }
}

impl Drop for Receiver {
    fn drop(&mut self) {
        self.terminate();
    }
}
