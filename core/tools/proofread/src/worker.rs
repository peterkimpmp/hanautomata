//! Bounded JSONL supervisor. Only the disposable child loads native code or submitted text.
//! Queues and frames are bounded; cancellation/EOF/timeout kill AND reap the child.
use crate::{Checker, Error, HunspellLexicon, Request};
use serde::Deserialize;
use serde_json::{json, Value};
use std::{io::{self, BufRead, BufReader, Read, Write}, path::Path,
    process::{Child, Command, Stdio}, sync::mpsc::{self, Receiver, SyncSender},
    thread, time::{Duration, Instant}};

const INPUT_LIMIT: usize = 65_536;
const OUTPUT_LIMIT: usize = 262_144;

#[derive(Deserialize)]
#[serde(tag = "command", rename_all = "snake_case", deny_unknown_fields)]
enum CommandFrame {
    Check { schema_version: u32, request: Request },
    Cancel { schema_version: u32, request_id: String },
    Shutdown { schema_version: u32 },
}

fn frame(reader: &mut impl BufRead, limit: usize) -> Result<Option<Vec<u8>>, Error> {
    let mut bytes = Vec::new();
    reader.take((limit + 1) as u64).read_until(b'\n', &mut bytes).map_err(|_| Error("pipe_read_failed"))?;
    if bytes.is_empty() { return Ok(None); }
    if bytes.len() > limit { return Err(Error("frame_too_large")); }
    if bytes.last() != Some(&b'\n') { return Err(Error("incomplete_frame")); }
    Ok(Some(bytes))
}

fn emit(value: &Value) -> Result<(), Error> {
    let mut out = io::stdout().lock();
    serde_json::to_writer(&mut out, value).map_err(|_| Error("stdout_write_failed"))?;
    writeln!(out).and_then(|_| out.flush()).map_err(|_| Error("stdout_write_failed"))
}

fn failure(id: Option<&str>, code: &str) -> Value {
    json!({"schema_version":1,"event":"error","request_id":id,"code":code})
}

struct Worker {
    process: Child,
    input: SyncSender<Vec<u8>>,
    output: Receiver<Result<Value, Error>>,
    peak_rss: u64,
}
impl Worker {
    fn start(directory: &Path, rules: bool, memory_mib: u64) -> Result<Self, Error> {
        let mut command = Command::new(std::env::current_exe().map_err(|_| Error("worker_path_failed"))?);
        command.arg("--dictionary").arg(directory).arg("--worker").arg("--memory-mib").arg(memory_mib.to_string());
        if rules { command.arg("--rules"); }
        command.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null());
        #[cfg(windows)] {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x08000000); // CREATE_NO_WINDOW
        }
        let mut process = command.spawn().map_err(|_| Error("worker_spawn_failed"))?;
        let mut stdin = process.stdin.take().ok_or(Error("worker_pipe_failed"))?;
        let stdout = process.stdout.take().ok_or(Error("worker_pipe_failed"))?;
        let (send, receive) = mpsc::sync_channel::<Vec<u8>>(1);
        thread::spawn(move || {
            while let Ok(bytes) = receive.recv() {
                if stdin.write_all(&bytes).and_then(|_| stdin.flush()).is_err() { break; }
            }
        });
        let (reply, output) = mpsc::sync_channel(1);
        thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            loop {
                let value = match frame(&mut reader, OUTPUT_LIMIT) {
                    Ok(Some(bytes)) => serde_json::from_slice(&bytes).map_err(|_| Error("invalid_worker_reply")),
                    Ok(None) => Err(Error("worker_exited")),
                    Err(e) => Err(e),
                };
                let failed = value.is_err();
                if reply.send(value).is_err() || failed { break; }
            }
        });
        Ok(Self { process, input: send, output, peak_rss: 0 })
    }
}
impl Drop for Worker {
    fn drop(&mut self) { let _ = self.process.kill(); let _ = self.process.wait(); }
}

pub fn serve(directory: &Path, rules: bool, timeout: Duration, memory_mib: u64) -> Result<(), Error> {
    let (send, input) = mpsc::sync_channel(2);
    thread::spawn(move || {
        let mut reader = io::stdin().lock();
        loop {
            let next = frame(&mut reader, INPUT_LIMIT);
            let stop = !matches!(next, Ok(Some(_)));
            if send.send(next).is_err() || stop { break; }
        }
    });
    emit(&json!({"schema_version":1,"event":"ready","timeout_ms":timeout.as_millis(),
        "memory_mib":memory_mib,"memory_policy":crate::resource::policy(),"rules":rules}))?;
    let mut worker: Option<Worker> = None;
    let mut pending: Option<(String, Instant)> = None;
    loop {
        match input.recv_timeout(Duration::from_millis(5)) {
            Ok(Ok(Some(bytes))) => {
                let cmd = serde_json::from_slice::<CommandFrame>(&bytes);
                match cmd {
                    Ok(CommandFrame::Check { schema_version: 1, request }) => {
                        if let Err(e) = crate::validate(&request) { emit(&failure(Some(&request.request_id), e.0))?; continue; }
                        if let Some((id, _)) = pending.take() {
                            worker = None;
                            emit(&json!({"schema_version":1,"event":"cancelled","request_id":id,"reason":"superseded"}))?;
                        }
                        if worker.is_none() {
                            match Worker::start(directory, rules, memory_mib) {
                                Ok(w) => worker = Some(w),
                                Err(e) => { emit(&failure(Some(&request.request_id), e.0))?; continue; }
                            }
                        }
                        let mut encoded = serde_json::to_vec(&request).map_err(|_| Error("serialization_failed"))?;
                        encoded.push(b'\n');
                        if worker.as_ref().unwrap().input.try_send(encoded).is_err() {
                            worker = None;
                            emit(&failure(Some(&request.request_id), "worker_unavailable"))?;
                        } else { pending = Some((request.request_id, Instant::now())); }
                    }
                    Ok(CommandFrame::Cancel { schema_version: 1, request_id }) => {
                        if pending.as_ref().is_some_and(|(id, _)| id == &request_id) {
                            worker = None; pending = None;
                            emit(&json!({"schema_version":1,"event":"cancelled","request_id":request_id,"reason":"requested"}))?;
                        }
                    }
                    Ok(CommandFrame::Shutdown { schema_version: 1 }) => break,
                    _ => emit(&failure(None, "invalid_command"))?,
                }
            }
            Ok(Ok(None)) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
            Ok(Err(e)) => { emit(&failure(None, e.0))?; break; }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
        if let Some((id, start)) = &pending {
            if start.elapsed() >= timeout {
                emit(&failure(Some(id), "timeout"))?;
                worker = None; pending = None; continue;
            }
            if let Some(w) = &mut worker {
                if let Some(rss) = crate::resource::resident_bytes(w.process.id()) {
                    w.peak_rss = w.peak_rss.max(rss);
                    if rss > memory_mib * 1024 * 1024 {
                        emit(&failure(Some(id), "memory_limit"))?;
                        worker = None; pending = None; continue;
                    }
                }
                if let Ok(result) = w.output.try_recv() {
                    match result {
                        Ok(mut value) if value["request_id"].as_str() == Some(id) => {
                            value["worker_peak_sampled_rss_bytes"] = json!(w.peak_rss);
                            emit(&value)?;
                        }
                        Ok(_) => { emit(&failure(Some(id), "invalid_worker_identity"))?; worker = None; }
                        Err(e) => { emit(&failure(Some(id), e.0))?; worker = None; }
                    }
                    pending = None;
                }
            }
        }
    }
    drop(worker);
    Ok(())
}

/// Runs only in the disposable process. Supported OS limits precede native initialization.
pub fn native_worker(directory: &Path, rules: bool, memory_mib: u64) -> Result<(), Error> {
    crate::resource::limit(memory_mib)?;
    let mut reader = io::stdin().lock();
    let mut checker = None;
    while let Some(bytes) = frame(&mut reader, INPUT_LIMIT)? {
        let request: Request = serde_json::from_slice(&bytes).map_err(|_| Error("invalid_request_json"))?;
        let start = Instant::now();
        if checker.is_none() {
            match HunspellLexicon::load_pinned(directory) {
                Ok(lexicon) => checker = Some(Checker::new(lexicon).with_rules(rules)),
                Err(e) => { emit(&failure(Some(&request.request_id), e.0))?; continue; }
            }
        }
        match checker.as_mut().unwrap().check(&request) {
            Ok(report) => emit(&json!({"schema_version":1,"event":"result","request_id":request.request_id,
                "report":report,"elapsed_ms":start.elapsed().as_secs_f64()*1000.0}))?,
            Err(e) => emit(&failure(Some(&request.request_id), e.0))?,
        }
    }
    Ok(())
}
