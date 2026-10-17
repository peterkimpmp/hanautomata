use hanautomata_assist::{store::Store, Reply, Request};
use std::{
    io::{self, BufRead, Write},
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

const MAX_REQUEST: usize = 16 * 1024;
fn emit(reply: &Reply) -> io::Result<()> {
    let mut out = io::stdout().lock();
    serde_json::to_writer(&mut out, reply)?;
    out.write_all(b"\n")?;
    out.flush()
}
fn run() -> io::Result<()> {
    let args: Vec<_> = std::env::args_os().collect();
    if args.len() == 2 && args[1] == "--version" {
        println!("hanautomata-assist {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    if args.len() != 3 || args[1] != "--store" {
        return emit(&Reply::error("", "invalid_request"));
    }
    let mut store = match Store::open(Path::new(&args[2])) {
        Ok(value) => value,
        Err(error) => return emit(&Reply::error("", error)),
    };
    let mut input = io::stdin().lock();
    loop {
        let mut frame = Vec::new();
        loop {
            let available = input.fill_buf()?;
            if available.is_empty() {
                if frame.is_empty() {
                    return Ok(());
                } else {
                    return emit(&Reply::error("", "invalid_request"));
                }
            }
            let end = available.iter().position(|b| *b == b'\n');
            let count = end.map_or(available.len(), |index| index + 1);
            if frame.len() + count > MAX_REQUEST {
                return emit(&Reply::error("", "invalid_request"));
            }
            frame.extend_from_slice(&available[..count]);
            input.consume(count);
            if end.is_some() {
                break;
            }
        }
        let reply = match serde_json::from_slice::<Request>(&frame) {
            Ok(request) => store.handle(
                request,
                SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs() / 86400,
            ),
            Err(_) => Reply::error("", "invalid_request"),
        };
        emit(&reply)?;
    }
}
fn main() {
    // Never print input, filesystem paths or serde parsing fragments into logs.
    if run().is_err() {
        std::process::exit(2);
    }
}
