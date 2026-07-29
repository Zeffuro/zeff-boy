use std::io::{self, BufRead, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

use anyhow::{Context, bail};
use serde_json::{Value, json};

pub(crate) fn call_live(addr: &str, mut request: Value) -> anyhow::Result<Value> {
    request["id"] = json!(1);
    let mut stream =
        connect_to_control(addr).with_context(|| format!("failed to connect to {addr}"))?;
    writeln!(stream, "{request}")?;
    stream.flush()?;

    let mut line = String::new();
    io::BufReader::new(stream).read_line(&mut line)?;
    let response: Value = serde_json::from_str(line.trim()).context("invalid live response")?;
    if response.get("ok").and_then(Value::as_bool) == Some(true) {
        Ok(response.get("result").cloned().unwrap_or(Value::Null))
    } else {
        let error = response
            .get("error")
            .and_then(Value::as_str)
            .unwrap_or("live command failed");
        bail!("{error}")
    }
}

fn connect_to_control(addr: &str) -> anyhow::Result<TcpStream> {
    let mut last_err = None;
    for addr in addr.to_socket_addrs()? {
        match TcpStream::connect_timeout(&addr, Duration::from_secs(2)) {
            Ok(stream) => {
                stream.set_read_timeout(Some(Duration::from_secs(5)))?;
                stream.set_write_timeout(Some(Duration::from_secs(5)))?;
                return Ok(stream);
            }
            Err(err) => last_err = Some(err),
        }
    }
    Err(last_err
        .map(anyhow::Error::from)
        .unwrap_or_else(|| anyhow::anyhow!("no resolved socket addresses")))
}
