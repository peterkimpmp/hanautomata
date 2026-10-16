use hanautomata_proofread::{Checker, Error, HunspellLexicon, Request};
use std::{
    io::{self, Read, Write},
    path::PathBuf,
    time::Instant,
};

fn run() -> Result<(), Error> {
    let mut args = std::env::args_os().skip(1);
    let mut directory = None;
    let mut json = false;
    let mut mode = "once";
    let mut rules = false;
    let mut timeout = 3000_u64;
    let mut memory = 512_u64;
    while let Some(arg) = args.next() {
        match arg.to_str().ok_or(Error("invalid_arguments"))? {
            "--dictionary" if directory.is_none() => {
                directory = Some(PathBuf::from(args.next().ok_or(Error("missing_dictionary_argument"))?))
            }
            "--json" if !json => json = true,
            "--serve" if mode == "once" => mode = "serve",
            "--worker" if mode == "once" => mode = "worker",
            "--rules" if !rules => rules = true,
            "--timeout-ms" => timeout = args.next().and_then(|s| s.to_str()?.parse().ok()).filter(|n| (1..=10_000).contains(n)).ok_or(Error("invalid_timeout"))?,
            "--memory-mib" => memory = args.next().and_then(|s| s.to_str()?.parse().ok()).filter(|n| (64..=1024).contains(n)).ok_or(Error("invalid_memory_limit"))?,
            "--version" => {
                println!("hanautomata-proofread {} (experimental)", env!("CARGO_PKG_VERSION"));
                return Ok(());
            }
            "--help" => {
                println!("Usage: hanautomata-proofread --dictionary DIR [--json | --serve] [--rules]\nRead UTF-8 text (or one JSON Request with --json) from stdin.\n--serve: bounded JSONL check/cancel/shutdown protocol, isolated native child.\n--timeout-ms 1..10000 (default 3000); --memory-mib 64..1024 (default 512).\nResource policy is reported by the ready event; macOS RSS is sampled, not a hard cap.\nExit 0: inspected; exit 2: invalid input/data. Unknown words need review.\nNo network, input logging, or document mutation. Limits are reported.");
                return Ok(());
            }
            _ => return Err(Error("invalid_arguments")),
        }
    }
    let directory = directory.ok_or(Error("dictionary_required"))?;
    // Hunspell's GNU Windows build opens narrow filenames. Resolve the directory
    // with Rust's Unicode-aware OS API, then give native code only ASCII names.
    // This is a standalone process and no threads exist yet; library callers'
    // working directories are never changed. Children inherit this directory.
    // Keep the original path on failure so missing data retains the supervisor's
    // ready/error/recovery protocol rather than failing before its ready event.
    #[cfg(windows)]
    let directory = if std::env::set_current_dir(&directory).is_ok() {
        PathBuf::from(".")
    } else {
        directory
    };
    if mode == "serve" { return hanautomata_proofread::worker::serve(&directory, rules, std::time::Duration::from_millis(timeout), memory); }
    if mode == "worker" { return hanautomata_proofread::worker::native_worker(&directory, rules, memory); }
    let mut bytes = Vec::new();
    io::stdin()
        .take(65_537)
        .read_to_end(&mut bytes)
        .map_err(|_| Error("stdin_read_failed"))?;
    if bytes.len() > 65_536 {
        return Err(Error("request_too_large"));
    }
    let request: Request = if json {
        serde_json::from_slice(&bytes).map_err(|_| Error("invalid_request_json"))?
    } else {
        Request {
            request_id: "cli-1".into(),
            document_id: "stdin".into(),
            revision: "1".into(),
            text: String::from_utf8(bytes).map_err(|_| Error("invalid_utf8"))?,
            protected_words: vec![],
        }
    };
    let start = Instant::now();
    let mut checker = Checker::new(HunspellLexicon::load_pinned(&directory)?).with_rules(rules);
    let load_ms = start.elapsed().as_secs_f64() * 1000.0;
    let start = Instant::now();
    let report = checker.check(&request)?;
    let check_ms = start.elapsed().as_secs_f64() * 1000.0;
    let mut value = serde_json::to_value(report).map_err(|_| Error("serialization_failed"))?;
    value["timing_ms"] = serde_json::json!({"load": load_ms, "check": check_ms});
    let out = io::stdout();
    let mut out = out.lock();
    serde_json::to_writer(&mut out, &value).map_err(|_| Error("stdout_write_failed"))?;
    writeln!(out).map_err(|_| Error("stdout_write_failed"))?;
    Ok(())
}

fn main() {
    if let Err(error) = run() {
        // Stable error codes only; do not echo private input or paths.
        let _ = writeln!(io::stderr(), "{}", serde_json::json!({"schema_version":1,"error":error.0}));
        std::process::exit(2);
    }
}
