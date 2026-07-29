mod args;
mod live_client;
mod protocol;
mod repo;
mod server;

fn main() {
    if let Err(err) = server::run() {
        eprintln!("zeff-mcp failed: {err:?}");
    }
}
