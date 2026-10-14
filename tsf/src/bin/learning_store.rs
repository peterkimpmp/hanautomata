//! Development stdio driver for the single-writer authority. No install, autostart, hooks or IPC listener.
//! The parent explicitly selects the file and owns both stdio pipes. This is not authenticated production IPC.
#[cfg(not(windows))]
fn main() { eprintln!("This development driver requires Windows."); std::process::exit(2); }

#[cfg(windows)]
fn main() { std::process::exit(driver::run()); }

#[cfg(windows)]
mod driver {
    use hanautomata_core::learning::{Authority, Error, LeaseKey, Request, MAX_REQUEST_BYTES};
    use serde::Deserialize;
    use serde_json::{json, Value};
    use std::{io::{self, BufRead, Write}, path::Path, time::{Duration, Instant}};
    use windows::Win32::System::Com::CoCreateGuid;

    #[derive(Deserialize)]
    #[serde(tag = "command", rename_all = "snake_case", deny_unknown_fields)]
    enum Command {
        Snapshot {}, Register {}, Clients {}, Resume { key: LeaseKey },
        Reconcile { keep: Vec<LeaseKey> }, Retire { client_id: String, lease_revision: u64 }, Apply { request: Request },
    }

    fn new_id() -> Result<String, Error> {
        Ok(format!("{:032x}", unsafe { CoCreateGuid() }.map_err(|_| Error::Io)?.to_u128()))
    }

    // One bounded newline-delimited command. An oversized/unclosed frame ends this development stream.
    // No unbounded read_until allocation, no draining an indefinitely growing malformed message.
    fn frame(input: &mut impl BufRead) -> io::Result<Option<Vec<u8>>> {
        let mut bytes = Vec::new();
        loop {
            let available = input.fill_buf()?;
            if available.is_empty() { return if bytes.is_empty() { Ok(None) } else { Err(io::ErrorKind::UnexpectedEof.into()) }; }
            let end = available.iter().position(|b| *b == b'\n');
            let count = end.map(|n| n + 1).unwrap_or(available.len());
            if bytes.len() + count > MAX_REQUEST_BYTES + 1 { return Err(io::ErrorKind::InvalidData.into()); }
            bytes.extend_from_slice(&available[..count]); input.consume(count);
            if end.is_some() { bytes.pop(); return Ok(Some(bytes)); }
        }
    }
    fn reply(output: &mut impl Write, value: &Value) -> io::Result<()> {
        serde_json::to_writer(&mut *output, value)?; output.write_all(b"\n")?; output.flush()
    }
    fn dispatch(owner: &mut Authority, command: Command) -> Result<Value, Error> {
        match command {
            Command::Snapshot {} => Ok(json!({"ok": true, "snapshot": owner.snapshot()?})),
            Command::Register {} => Ok(json!({"ok": true, "lease": owner.register(&new_id()?)?})),
            Command::Clients {} => Ok(json!({"ok": true, "clients": owner.clients()?})),
            Command::Resume { key } => {
                if !owner.clients()?.iter().any(|client| client.key.client_id == key.client_id && client.key.lease_revision == key.lease_revision) {
                    return Err(Error::ExpiredLease);
                }
                Ok(json!({"ok": true, "lease": owner.register(&key.client_id)?}))
            },
            Command::Reconcile { keep } => Ok(json!({"ok": true, "retired": owner.retire_absent(&keep)?})),
            Command::Retire { client_id, lease_revision } => Ok(json!({"ok": true, "retired": owner.retire(&client_id, lease_revision)?})),
            Command::Apply { request } => Ok(json!({"ok": true, "reply": owner.apply(&request)?})),
        }
    }
    fn serve(owner: &mut Authority, input: &mut impl BufRead, output: &mut impl Write) -> io::Result<i32> {
        loop {
            let bytes = match frame(input) {
                Ok(Some(bytes)) => bytes, Ok(None) => return Ok(0),
                Err(_) => { reply(output, &json!({"ok": false, "error": Error::InvalidRequest}))?; return Ok(2); },
            };
            let result = serde_json::from_slice(&bytes).map_err(|_| Error::InvalidRequest).and_then(|command| dispatch(owner, command));
            match result {
                Ok(value) => reply(output, &value)?,
                Err(error) => {
                    reply(output, &json!({"ok": false, "error": error}))?;
                    if owner.snapshot().is_err() { return Ok(2); }
                },
            }
        }
    }
    pub(super) fn run() -> i32 {
        let args: Vec<_> = std::env::args_os().skip(1).collect();
        let [mode, flag, file] = args.as_slice() else { eprintln!("usage: hanautomata-learning-store --development-stdio --store <dedicated-file>"); return 2; };
        if mode != "--development-stdio" || flag != "--store" { eprintln!("invalid development arguments"); return 2; }
        // The previous owned process may have exited before Windows releases its file lock.
        let deadline = Instant::now() + Duration::from_millis(500);
        let mut owner = match new_id().and_then(|id| loop {
            match Authority::open(Path::new(file), &id) {
                Err(Error::Busy) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(10)),
                result => break result,
            }
        }) {
            Ok(owner) => owner, Err(error) => { eprintln!("{error}"); return 2; },
        };
        match serve(&mut owner, &mut io::stdin().lock(), &mut io::stdout().lock()) {
            Ok(code) => code, Err(_) => { eprintln!("learning store I/O failed"); 2 },
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use std::io::{BufReader, Cursor};
        #[test]
        fn bounded_frames_do_not_consume_the_next_request() {
            let mut input = BufReader::with_capacity(2, Cursor::new(b"{}\n{ }\n"));
            assert_eq!(frame(&mut input).unwrap().unwrap(), b"{}");
            assert_eq!(frame(&mut input).unwrap().unwrap(), b"{ }");
            assert!(frame(&mut input).unwrap().is_none());
        }
        #[test]
        fn oversized_and_unterminated_frames_are_rejected() {
            assert!(frame(&mut Cursor::new(vec![b'x'; MAX_REQUEST_BYTES + 2])).is_err());
            assert!(frame(&mut Cursor::new(b"{}".to_vec())).is_err());
            let mut exact = vec![b' '; MAX_REQUEST_BYTES]; exact.push(b'\n');
            assert_eq!(frame(&mut Cursor::new(exact)).unwrap().unwrap().len(), MAX_REQUEST_BYTES);
        }
        #[test]
        fn unknown_commands_and_extra_fields_cannot_be_ignored() {
            for bytes in [br#"{"command":"register","client_id":"chosen"}"#.as_slice(),
                br#"{"command":"replace","entries":[]}"#, br#"{"command":"retire","client_id":"x"}"#] {
                assert!(serde_json::from_slice::<Command>(bytes).is_err());
            }
        }
    }
}
