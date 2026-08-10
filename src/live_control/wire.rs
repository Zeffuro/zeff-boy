use std::sync::mpsc::{self, Sender};

use serde_json::{Value, json};

use super::RESPONSE_TIMEOUT;
use super::parse::parse_wire_request;
use super::types::{LiveReply, LiveRequest};

pub(super) fn dispatch_wire_request(line: &str, tx: &Sender<LiveRequest>) -> (Value, LiveReply) {
    let parsed = match parse_wire_request(line) {
        Ok(parsed) => parsed,
        Err(err) => return (Value::Null, LiveReply::error(err)),
    };

    let id = parsed.id.unwrap_or(Value::Null);
    let (response_tx, response_rx) = mpsc::channel();
    let request = LiveRequest {
        command: parsed.command,
        response_tx,
    };

    if tx.send(request).is_err() {
        return (id, LiveReply::error("live control is shutting down"));
    }

    match response_rx.recv_timeout(RESPONSE_TIMEOUT) {
        Ok(response) => (id, response),
        Err(_) => (id, LiveReply::error("live control response timed out")),
    }
}

pub(super) fn wire_reply(id: Value, reply: LiveReply) -> Value {
    match reply {
        LiveReply::Ok(result) => json!({
            "id": id,
            "ok": true,
            "result": result,
        }),
        LiveReply::Error(error) => wire_error(id, error),
    }
}

pub(super) fn wire_error(id: Value, error: impl Into<String>) -> Value {
    json!({
        "id": id,
        "ok": false,
        "error": error.into(),
    })
}
