use crate::{valid_id, Library, Reply, Request};
use std::{
    fs::{self, File, OpenOptions, TryLockError},
    io::{Read, Write},
    path::{Path, PathBuf},
    thread,
    time::{Duration, Instant},
};

pub const MAX_FILE: u64 = 1024 * 1024;
const LOCK_WAIT: Duration = Duration::from_millis(500);
pub struct Store {
    library: Library,
    path: PathBuf,
    temporary: PathBuf,
    _lock: File,
}

fn regular_or_missing(path: &Path) -> bool {
    match fs::symlink_metadata(path) {
        Ok(m) => m.is_file() && !m.file_type().is_symlink(),
        Err(e) => e.kind() == std::io::ErrorKind::NotFound,
    }
}
fn private_options() -> OpenOptions {
    let mut options = OpenOptions::new();
    options.read(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options
}
// A lock can stay held briefly after its holder let go: Windows unlocks a killed helper's file when resources
// allow, and on Unix a child forked by another thread keeps the flock until it execs. Wait that out; a live
// writer such as another window still gets store_unavailable well inside the clients' 3 s request timeout.
fn lock_exclusive(lock: &File) -> Result<(), &'static str> {
    let deadline = Instant::now() + LOCK_WAIT;
    loop {
        match lock.try_lock() {
            Ok(()) => return Ok(()),
            Err(TryLockError::WouldBlock) if Instant::now() < deadline => thread::sleep(Duration::from_millis(5)),
            Err(_) => return Err("store_unavailable"),
        }
    }
}
impl Store {
    pub fn open(path: &Path) -> Result<Self, &'static str> {
        if !path.is_absolute() {
            return Err("store_unavailable");
        }
        let parent = path.parent().ok_or("store_unavailable")?;
        fs::create_dir_all(parent).map_err(|_| "store_unavailable")?;
        let lock_path = path.with_extension("json.lock");
        let temporary = path.with_extension("json.tmp");
        if !regular_or_missing(path) || !regular_or_missing(&lock_path) || !regular_or_missing(&temporary) {
            return Err("store_unavailable");
        }
        let lock = private_options()
            .create(true)
            .truncate(false)
            .open(lock_path)
            .map_err(|_| "store_unavailable")?;
        lock_exclusive(&lock)?;
        // Under the exclusive OS lock, a temporary file can only be from an interrupted previous save.
        if temporary.exists() {
            fs::remove_file(&temporary).map_err(|_| "store_unavailable")?;
        }
        let library = match File::open(path) {
            Ok(file) => {
                let mut bytes = vec![];
                file.take(MAX_FILE + 1)
                    .read_to_end(&mut bytes)
                    .map_err(|_| "store_unavailable")?;
                if bytes.len() as u64 > MAX_FILE {
                    return Err("store_unavailable");
                }
                let value: Library = serde_json::from_slice(&bytes).map_err(|_| "store_unavailable")?;
                value.validate()?;
                value
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Library::default(),
            Err(_) => return Err("store_unavailable"),
        };
        Ok(Self {
            library,
            path: path.into(),
            temporary,
            _lock: lock,
        })
    }
    fn persist(&self, library: &Library) -> Result<(), &'static str> {
        let bytes = serde_json::to_vec(library).map_err(|_| "store_unavailable")?;
        if bytes.len() as u64 > MAX_FILE || !regular_or_missing(&self.path) {
            return Err("store_unavailable");
        }
        let mut file = private_options()
            .create_new(true)
            .open(&self.temporary)
            .map_err(|_| "store_unavailable")?;
        let written = file.write_all(&bytes).and_then(|_| file.sync_all());
        drop(file);
        let saved = written.and_then(|_| fs::rename(&self.temporary, &self.path));
        if saved.is_err() {
            let _ = fs::remove_file(&self.temporary);
            return Err("store_unavailable");
        }
        #[cfg(unix)]
        if let Some(parent) = self.path.parent() {
            if let Ok(dir) = File::open(parent) {
                let _ = dir.sync_all();
            }
        }
        Ok(())
    }
    pub fn handle(&mut self, request: Request, day: u64) -> Reply {
        if request.schema_version != 1 || !valid_id(&request.request_id) {
            return Reply::error("", "invalid_request");
        }
        match self.execute(&request, day) {
            Ok((items, undo_id)) => Reply {
                schema_version: 1,
                request_id: request.request_id,
                ok: true,
                enabled: self.library.enabled,
                items,
                error: String::new(),
                undo_id,
            },
            Err(error) => Reply::error(&request.request_id, error),
        }
    }
    fn execute(&mut self, r: &Request, day: u64) -> Result<(Vec<crate::Entry>, String), &'static str> {
        if r.command == "list" {
            return Ok((self.library.list(), String::new()));
        }
        if r.command == "query" {
            return Ok((
                self.library.query(r.text.as_deref().ok_or("invalid_request")?, day),
                String::new(),
            ));
        }
        let mut next = self.library.clone();
        let mut token = String::new();
        match r.command.as_str() {
            "add" => {
                next.add(r.text.as_deref().ok_or("invalid_request")?)?;
            }
            "set_enabled" => next.set_enabled(r.enabled.ok_or("invalid_request")?),
            "copied" => {
                token = next.copied(r.entry_id.as_deref().ok_or("invalid_request")?, day)?;
            }
            "undo" => next.undo(r.undo_id.as_deref().ok_or("invalid_request")?)?,
            "delete" => next.delete(r.entry_id.as_deref().ok_or("invalid_request")?)?,
            "clear" => next.clear(),
            _ => return Err("invalid_request"),
        }
        self.persist(&next)?;
        self.library = next;
        Ok((self.library.list(), token))
    }
}
