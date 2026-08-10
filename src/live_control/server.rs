use std::io::{BufRead, BufReader, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;

use serde_json::Value;

use super::types::LiveRequest;
use super::wire::{dispatch_wire_request, wire_error, wire_reply};
use super::{DEFAULT_ADDR, ENV_VAR};

pub(super) fn start_from_env() -> Option<(Receiver<LiveRequest>, Option<SocketAddr>)> {
    let addr = configured_addr()?;

    if !addr.ip().is_loopback() {
        log::warn!("{ENV_VAR} must bind to a loopback address; refusing live control on {addr}");
        return None;
    }

    let listener = match TcpListener::bind(addr) {
        Ok(listener) => listener,
        Err(err) => {
            log::warn!("Failed to start live control on {addr}: {err}");
            return None;
        }
    };

    let actual_addr = listener.local_addr().ok();
    let (tx, rx) = mpsc::channel();
    spawn_listener(listener, tx);

    if let Some(addr) = actual_addr {
        log::info!("Live control listening on {addr}");
    }

    Some((rx, actual_addr))
}

fn configured_addr() -> Option<SocketAddr> {
    let value = std::env::var(ENV_VAR).ok()?;
    let trimmed = value.trim();
    if trimmed.is_empty() || matches!(trimmed, "0" | "false" | "False" | "off" | "Off" | "no") {
        return None;
    }

    let addr = if matches!(trimmed, "1" | "true" | "True" | "on" | "On" | "yes") {
        DEFAULT_ADDR
    } else {
        trimmed
    };

    match addr.parse() {
        Ok(addr) => Some(addr),
        Err(err) => {
            log::warn!("Ignoring invalid {ENV_VAR} value {trimmed:?}: {err}");
            None
        }
    }
}

fn spawn_listener(listener: TcpListener, tx: Sender<LiveRequest>) {
    if let Err(err) = thread::Builder::new()
        .name("zeff-live-control".into())
        .spawn(move || {
            for stream in listener.incoming() {
                let Ok(stream) = stream else {
                    continue;
                };
                let tx = tx.clone();
                let _ = thread::Builder::new()
                    .name("zeff-live-control-client".into())
                    .spawn(move || handle_client(stream, tx));
            }
        })
    {
        log::warn!("Failed to spawn live control listener: {err}");
    }
}

fn handle_client(mut stream: TcpStream, tx: Sender<LiveRequest>) {
    let Ok(reader_stream) = stream.try_clone() else {
        let _ = writeln!(
            stream,
            "{}",
            wire_error(Value::Null, "failed to clone stream")
        );
        return;
    };
    let mut reader = BufReader::new(reader_stream);
    let mut line = String::new();

    loop {
        line.clear();
        let read = match reader.read_line(&mut line) {
            Ok(read) => read,
            Err(err) => {
                let _ = writeln!(
                    stream,
                    "{}",
                    wire_error(Value::Null, format!("failed to read request: {err}"))
                );
                break;
            }
        };
        if read == 0 {
            break;
        }

        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let (id, response) = dispatch_wire_request(trimmed, &tx);
        let wire = wire_reply(id, response);
        if writeln!(stream, "{wire}").is_err() {
            break;
        }
    }
}
