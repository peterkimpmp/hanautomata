//! Single-writer learning transactions. Platform brokers own this object off the key callback.
//! This is separate from the legacy, whole-dictionary `Store`; it does not install a broker.
#[path = "learning_client.rs"]
pub mod client;

use crate::{LearnedWord, WordPreferences, CAPACITY};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{collections::HashSet, fs::{self, File, OpenOptions, TryLockError}, io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf}, sync::atomic::{AtomicU64, Ordering}, time::Duration};

pub const MAX_CLIENTS: usize = 256;
pub const MAX_REQUEST_BYTES: usize = 4096;
pub const MAX_STATE_BYTES: usize = 1_048_576;
const MAX_FENCES: usize = 256;
static NEXT_TEMPORARY: AtomicU64 = AtomicU64::new(1);
#[cfg(test)]
thread_local! {
    static RETRY_OBSERVER: std::cell::RefCell<Option<std::sync::Arc<std::sync::atomic::AtomicUsize>>> = const { std::cell::RefCell::new(None) };
}
fn retry_io(delay: u64) {
    #[cfg(test)] RETRY_OBSERVER.with(|observer| {
        if let Some(count) = observer.borrow().as_ref() { count.fetch_add(1, Ordering::Relaxed); }
    });
    std::thread::sleep(Duration::from_millis(delay));
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Version { pub store_id: String, pub generation: u64, pub revision: u64 }
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Lease { pub client_id: String, pub lease_revision: u64, pub version: Version, pub next_sequence: u64 }
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LeaseKey { pub client_id: String, pub lease_revision: u64 }
#[derive(Clone, Debug, Serialize)]
pub struct ClientInfo { pub key: LeaseKey, pub sequence: u64, pub committed_revision: u64,
    #[serde(skip_serializing_if = "LeaseLifetime::is_unmanaged")]
    pub lifetime: LeaseLifetime }
/// Trusted broker policy, never supplied by an input client. Unmanaged/older leases require explicit management.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LeaseLifetime { #[default] Unmanaged, WindowsMutexV1 }
impl LeaseLifetime { fn is_unmanaged(&self) -> bool { *self == Self::Unmanaged } }
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum Operation {
    Learn { raw: String, keys: String, korean: bool, context: String },
    Forget { raw: String, keys: String },
    Reset {},
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub store_id: String, pub client_id: String, pub lease_revision: u64,
    /// Capture generation when the user acts; never automatically re-stamp or re-lease a queued action.
    /// A stale delete is rejected, not completed: present the current snapshot and request a new confirmation.
    pub sequence: u64, pub generation: u64, pub operation: Operation,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Reply {
    pub version: Version, pub committed_revision: u64, pub sequence: u64, pub changed: bool, pub duplicate: bool,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Snapshot { pub version: Version, pub entries: Vec<LearnedWord>, pub recovery: Recovery }
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
pub struct Recovery { pub retained_pending: usize, pub removed_pending: usize }
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Error { Io, Busy, InvalidState, MissingState, InvalidRequest, WrongStore, UnknownClient, ExpiredLease,
    TooManyClients, OutOfOrder, Superseded, SequenceConflict, StaleGeneration, Exhausted, Faulted, ExternalChange }
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { write!(f, "learning store: {self:?}") }
}
impl std::error::Error for Error {}
type Result<T> = std::result::Result<T, Error>;

const PREFIX: &[u8] = b"Hanautomata.TsfLearning/1\n";
// V2 adds managed-client lifetime metadata. Once all such leases are drained, the exact V1 shape
// is sufficient again; preserve words, fences, identities and revisions when returning to V1.
const MANAGED_PREFIX: &[u8] = b"Hanautomata.TsfLearning/2\n";
const OWNER_PREFIX: &str = "Hanautomata.TsfLearningOwner/1\n";

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Client {
    id: String, lease_revision: u64, sequence: u64, fingerprint: Option<String>,
    committed_revision: u64, changed: bool,
    #[serde(default, skip_serializing_if = "LeaseLifetime::is_unmanaged")]
    lifetime: LeaseLifetime,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Fence { generation: u64, word: Option<String> }
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct State { version: Version, entries: Vec<LearnedWord>, clients: Vec<Client>, fences: Vec<Fence> }

impl std::fmt::Debug for Operation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self { Self::Learn { .. } => "Learn { redacted }", Self::Forget { .. } => "Forget { redacted }", Self::Reset {} => "Reset" })
    }
}
impl std::fmt::Debug for Request {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Request").field("sequence", &self.sequence).field("generation", &self.generation)
            .field("operation", &self.operation).finish_non_exhaustive()
    }
}
impl std::fmt::Debug for Snapshot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Snapshot").field("version", &self.version).field("entry_count", &self.entries.len()).finish_non_exhaustive()
    }
}
impl std::fmt::Debug for State {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("State").field("version", &self.version).field("entry_count", &self.entries.len())
            .field("client_count", &self.clients.len()).finish_non_exhaustive()
    }
}

fn hex(bytes: &[u8]) -> String { bytes.iter().map(|b| format!("{b:02x}")).collect() }
fn valid_hex(value: &str, size: usize) -> bool {
    value.len() == size && value.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn valid_id(value: &str) -> bool { valid_hex(value, 32) && value.bytes().any(|b| b != b'0') }
fn word_fingerprint(raw: &str, keys: &str) -> Result<String> {
    let (identity, _, _) = crate::prefs::normalize(raw, keys).ok_or(Error::InvalidRequest)?;
    Ok(hex(&Sha256::digest(identity.as_bytes())))
}
fn suffix(path: &Path, value: &str) -> PathBuf {
    let mut name = path.as_os_str().to_owned(); name.push(value); PathBuf::from(name)
}
fn no_link(path: &Path) -> Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => Err(Error::InvalidState),
        Ok(_) => Ok(()), Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()), Err(_) => Err(Error::Io),
    }
}
fn read_blob(path: &Path) -> Result<Option<Vec<u8>>> {
    no_link(path)?;
    let mut attempt = 0;
    let file = loop {
        match File::open(path) {
            Ok(file) => break file, Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) if cfg!(windows) && matches!(e.raw_os_error(), Some(5 | 32)) && attempt < 3 => {
                retry_io([10, 20, 40][attempt]); attempt += 1;
            },
            Err(_) => return Err(Error::Io),
        }
    };
    let mut data = Vec::new();
    file.take(MAX_STATE_BYTES as u64 + 1).read_to_end(&mut data).map_err(|_| Error::Io)?;
    if data.len() > MAX_STATE_BYTES { return Err(Error::InvalidState); }
    Ok(Some(data))
}
fn decode_state(data: &[u8]) -> Result<State> {
    let (rest, legacy) = if let Some(rest) = data.strip_prefix(PREFIX) { (rest, true) }
        else { (data.strip_prefix(MANAGED_PREFIX).ok_or(Error::InvalidState)?, false) };
    if rest.len() < 65 || rest[64] != b'\n' { return Err(Error::InvalidState); }
    let checksum = std::str::from_utf8(&rest[..64]).map_err(|_| Error::InvalidState)?;
    let body = &rest[65..];
    if !valid_hex(checksum, 64) || hex(&Sha256::digest(body)) != checksum { return Err(Error::InvalidState); }
    let state: State = serde_json::from_slice(body).map_err(|_| Error::InvalidState)?;
    if legacy {
        // A V1 reader rejects *any* lifetime field, even "unmanaged". Do not accept a V2 body
        // disguised as V1 and then quietly rewrite it into a different compatibility contract.
        let shape: serde_json::Value = serde_json::from_slice(body).map_err(|_| Error::InvalidState)?;
        if shape["clients"].as_array().ok_or(Error::InvalidState)?.iter().any(|client| client.get("lifetime").is_some()) {
            return Err(Error::InvalidState);
        }
    }
    state.validate()?; Ok(state)
}
fn encode_state(state: &State) -> Result<Vec<u8>> {
    state.validate()?;
    let body = serde_json::to_vec(state).map_err(|_| Error::InvalidState)?;
    let prefix = if state.clients.iter().any(|client| client.lifetime != LeaseLifetime::Unmanaged) { MANAGED_PREFIX } else { PREFIX };
    let mut data = prefix.to_vec(); data.extend_from_slice(hex(&Sha256::digest(&body)).as_bytes());
    data.push(b'\n'); data.extend_from_slice(&body);
    if data.len() > MAX_STATE_BYTES { return Err(Error::InvalidState); } Ok(data)
}
impl State {
    fn validate(&self) -> Result<()> {
        if !valid_id(&self.version.store_id) || self.version.generation == 0 || self.version.revision == 0
            || self.version.generation > self.version.revision || self.entries.len() > CAPACITY
            || self.clients.len() > MAX_CLIENTS || self.fences.len() > MAX_FENCES { return Err(Error::InvalidState); }
        if self.fences.len() as u64 != (self.version.generation - 1).min(MAX_FENCES as u64) { return Err(Error::InvalidState); }
        for (index, fence) in self.fences.iter().enumerate() {
            let expected = self.version.generation - self.fences.len() as u64 + index as u64 + 1;
            if fence.generation != expected || fence.word.as_ref().is_some_and(|word| !valid_hex(word, 64)) { return Err(Error::InvalidState); }
        }
        let mut identities = HashSet::new(); let mut last_sequence = 0;
        for word in &self.entries {
            let Some((_, raw, keys)) = crate::prefs::normalize(&word.raw, &word.keys) else { return Err(Error::InvalidState); };
            if raw != word.raw || keys != word.keys || !crate::prefs::valid_context(&word.context)
                || word.sequence <= last_sequence || !identities.insert((&word.raw, &word.keys, &word.context))
                || [word.korean_weight, word.english_weight].iter().any(|w| !w.is_finite() || *w < 0.0 || *w > 16.0)
                || word.korean_weight + word.english_weight == 0.0 { return Err(Error::InvalidState); }
            last_sequence = word.sequence;
        }
        let mut ids = HashSet::new();
        for client in &self.clients {
            if !valid_id(&client.id) || !ids.insert(&client.id) || client.lease_revision == 0
                || client.lease_revision > client.committed_revision || client.committed_revision > self.version.revision
                || client.sequence > client.committed_revision - client.lease_revision
                || (client.sequence == 0 && (client.fingerprint.is_some() || client.changed || client.committed_revision != client.lease_revision))
                || (client.sequence != 0 && (client.committed_revision == client.lease_revision
                    || !client.fingerprint.as_ref().is_some_and(|value| valid_hex(value, 64)))) { return Err(Error::InvalidState); }
        }
        Ok(())
    }
    fn advance(&mut self) -> Result<()> {
        self.version.revision = self.version.revision.checked_add(1).ok_or(Error::Exhausted)?; Ok(())
    }
    fn fence(&mut self, word: Option<String>) -> Result<()> {
        self.version.generation = self.version.generation.checked_add(1).ok_or(Error::Exhausted)?;
        // A reset already fences every older request; old per-word hashes no longer serve a purpose.
        if word.is_none() { for fence in &mut self.fences { fence.word = None; } }
        self.fences.push(Fence { generation: self.version.generation, word });
        if self.fences.len() > MAX_FENCES { self.fences.remove(0); } Ok(())
    }
    fn accepts_generation(&self, request: &Request) -> bool {
        if request.generation == self.version.generation { return true; }
        if request.generation > self.version.generation { return false; }
        let Operation::Learn { raw, keys, .. } = &request.operation else { return false; };
        let Ok(word) = word_fingerprint(raw, keys) else { return false; };
        let Some(first) = self.fences.first() else { return false; };
        if request.generation < first.generation - 1 { return false; }
        self.fences.iter().filter(|f| f.generation > request.generation).all(|f| f.word.as_ref().is_some_and(|deleted| *deleted != word))
    }
}

// Only this exact database basename plus the generated suffix grammar is reserved for pending files.
// No glob deletion, recursive removal, symlink traversal, or deletion of another filename pattern.
fn prune_pending(path: &Path) -> Result<Recovery> {
    let parent = path.parent().ok_or(Error::InvalidRequest)?;
    let base = path.file_name().and_then(|name| name.to_str()).ok_or(Error::InvalidRequest)?;
    let prefix = format!("{base}.pending-"); let mut candidates = Vec::new();
    for (count, item) in fs::read_dir(parent).map_err(|_| Error::Io)?.enumerate() {
        if count >= 4096 { return Err(Error::Io); }
        let item = item.map_err(|_| Error::Io)?;
        if !item.file_type().map_err(|_| Error::Io)?.is_file() { continue; }
        let name = item.file_name(); let Some(name) = name.to_str() else { continue; };
        let Some(tail) = name.strip_prefix(&prefix) else { continue; };
        let parts: Vec<_> = tail.split('-').collect();
        if parts.len() != 4 || !valid_id(parts[0]) || parts[1..].iter().any(|part| part.is_empty() || !part.bytes().all(|b| b.is_ascii_digit())) { continue; }
        let (Ok(revision), Ok(pid), Ok(counter)) = (parts[1].parse::<u64>(), parts[2].parse::<u32>(), parts[3].parse::<u64>()) else { continue; };
        if revision == 0 || pid == 0 || counter == 0 { continue; }
        let candidate = item.path();
        if candidate.parent() != Some(parent) { return Err(Error::InvalidState); }
        candidates.push(candidate);
    }
    let mut removed_pending = 0;
    for candidate in candidates {
        fs::remove_file(candidate).map_err(|_| Error::Io)?; removed_pending += 1;
    }
    Ok(Recovery { retained_pending: 0, removed_pending })
}

/// One owner for an explicitly chosen, dedicated learning database. IDs are identifiers, not credentials.
/// The platform must mint new store/client IDs and authenticate peers; callers cannot select a file over IPC.
/// The lock is held for this object's lifetime. Errors after a write attempt require reopening before further use.
pub struct Authority {
    path: PathBuf, _owner: File, state: State, disk_hash: Option<String>, faulted: bool, recovery: Recovery,
    #[cfg(test)] injection: Option<(Stage, bool)>,
}
impl std::fmt::Debug for Authority {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Authority").field("state", &self.state).field("faulted", &self.faulted).finish_non_exhaustive()
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Stage { BeforeWrite, AfterSync, AfterReplace }
impl Authority {
    pub fn open(path: impl AsRef<Path>, new_store_id: &str) -> Result<Self> {
        let path = path.as_ref();
        let file_name = path.file_name().ok_or(Error::InvalidRequest)?;
        let parent = path.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or_else(|| Path::new("."));
        fs::create_dir_all(parent).map_err(|_| Error::Io)?;
        let path = parent.canonicalize().map_err(|_| Error::Io)?.join(file_name);
        let owner_path = suffix(&path, ".owner.lock"); no_link(&owner_path)?;
        let mut options = OpenOptions::new(); options.read(true).write(true).create(true).truncate(false);
        #[cfg(windows)] { use std::os::windows::fs::OpenOptionsExt; options.share_mode(0x1 | 0x2); } // no FILE_SHARE_DELETE
        let mut owner = options.open(&owner_path).map_err(|_| Error::Io)?;
        owner.try_lock().map_err(|e| match e { TryLockError::WouldBlock => Error::Busy, TryLockError::Error(_) => Error::Io })?;
        let mut marker = Vec::new();
        (&mut owner).take(128).read_to_end(&mut marker).map_err(|_| Error::Io)?;
        let data = read_blob(&path)?;
        let state = match data.as_ref() {
            Some(data) => decode_state(data)?,
            None => {
                if !marker.is_empty() { return Err(Error::MissingState); }
                if !valid_id(new_store_id) { return Err(Error::InvalidRequest); }
                State { version: Version { store_id: new_store_id.into(), generation: 1, revision: 1 }, entries: vec![], clients: vec![], fences: vec![] }
            },
        };
        let expected = format!("{OWNER_PREFIX}{}", state.version.store_id);
        if !marker.is_empty() && marker != expected.as_bytes() { return Err(Error::InvalidState); }
        let mut authority = Self { path, _owner: owner, state, disk_hash: data.as_ref().map(|bytes| hex(&Sha256::digest(bytes))),
            faulted: false, recovery: Recovery::default(), #[cfg(test)] injection: None };
        authority.recovery = prune_pending(&authority.path)?;
        if data.is_none() { authority.commit(authority.state.clone())?; }
        if marker.is_empty() {
            authority._owner.seek(SeekFrom::Start(0)).map_err(|_| Error::Io)?;
            authority._owner.write_all(expected.as_bytes()).map_err(|_| Error::Io)?;
            authority._owner.sync_all().map_err(|_| Error::Io)?;
        }
        Ok(authority)
    }
    fn ready(&self) -> Result<()> { if self.faulted { Err(Error::Faulted) } else { Ok(()) } }
    pub fn snapshot(&self) -> Result<Snapshot> {
        self.ready()?; Ok(Snapshot { version: self.state.version.clone(), entries: self.state.entries.clone(), recovery: self.recovery })
    }
    /// The broker registers a fresh, authenticated client before accepting requests. It must retire known-disconnected
    /// clients explicitly when capacity is reached. A repeated ID recovers that same lease after an uncertain reply;
    /// trusted management only, never interpret ID knowledge as authentication or replay under a new lease.
    pub fn register(&mut self, new_client_id: &str) -> Result<Lease> {
        self.register_with_lifetime(new_client_id, LeaseLifetime::Unmanaged)
    }
    /// The broker must own the declared lifetime guard before registration and keep it through all writes.
    /// A retry cannot promote an old/unmanaged lease or change its reclamation contract.
    pub fn register_with_lifetime(&mut self, new_client_id: &str, lifetime: LeaseLifetime) -> Result<Lease> {
        self.ready()?;
        if !valid_id(new_client_id) { return Err(Error::InvalidRequest); }
        if let Some(client) = self.state.clients.iter().find(|c| c.id == new_client_id) {
            if client.lifetime != lifetime { return Err(Error::InvalidRequest); }
            return Ok(Lease { client_id: client.id.clone(), lease_revision: client.lease_revision, version: self.state.version.clone(),
                next_sequence: client.sequence.checked_add(1).ok_or(Error::Exhausted)? });
        }
        if self.state.clients.len() >= MAX_CLIENTS { return Err(Error::TooManyClients); }
        let mut proposed = self.state.clone(); proposed.advance()?;
        let lease_revision = proposed.version.revision;
        proposed.clients.push(Client { id: new_client_id.into(), lease_revision, sequence: 0, fingerprint: None,
            committed_revision: lease_revision, changed: false, lifetime });
        self.commit(proposed)?;
        Ok(Lease { client_id: new_client_id.into(), lease_revision, version: self.state.version.clone(), next_sequence: 1 })
    }
    pub fn retire(&mut self, client_id: &str, lease_revision: u64) -> Result<bool> {
        self.ready()?;
        let Some(index) = self.state.clients.iter().position(|c| c.id == client_id) else { return Ok(false); };
        if self.state.clients[index].lease_revision != lease_revision { return Err(Error::ExpiredLease); }
        let mut proposed = self.state.clone(); proposed.clients.remove(index); proposed.advance()?; self.commit(proposed)?; Ok(true)
    }
    /// Trusted broker management only. Reconcile after authenticating reconnects and a reconnect grace period.
    pub fn clients(&self) -> Result<Vec<ClientInfo>> {
        self.ready()?;
        Ok(self.state.clients.iter().map(|client| ClientInfo { key: LeaseKey { client_id: client.id.clone(), lease_revision: client.lease_revision },
            sequence: client.sequence, committed_revision: client.committed_revision, lifetime: client.lifetime }).collect())
    }
    pub fn retire_absent(&mut self, keep: &[LeaseKey]) -> Result<usize> {
        self.ready()?;
        if keep.len() > MAX_CLIENTS { return Err(Error::InvalidRequest); }
        let mut ids = HashSet::new();
        for key in keep {
            if !ids.insert(&key.client_id) { return Err(Error::InvalidRequest); }
            if !self.state.clients.iter().any(|c| c.id == key.client_id && c.lease_revision == key.lease_revision) { return Err(Error::ExpiredLease); }
        }
        let removed = self.state.clients.len() - keep.len();
        if removed == 0 { return Ok(0); }
        let mut proposed = self.state.clone(); proposed.clients.retain(|c| ids.contains(&c.id));
        proposed.advance()?; self.commit(proposed)?; Ok(removed)
    }
    pub fn apply(&mut self, request: &Request) -> Result<Reply> {
        self.ready()?; request.validate()?;
        if request.store_id != self.state.version.store_id { return Err(Error::WrongStore); }
        let index = self.state.clients.iter().position(|c| c.id == request.client_id).ok_or(Error::UnknownClient)?;
        let client = &self.state.clients[index];
        if request.lease_revision != client.lease_revision { return Err(Error::ExpiredLease); }
        let fingerprint = request.fingerprint();
        // Check an exact committed retry before the generation barrier: a reset changes its own generation.
        if request.sequence == client.sequence {
            if client.fingerprint.as_ref() != Some(&fingerprint) { return Err(Error::SequenceConflict); }
            return Ok(Reply { version: self.state.version.clone(), committed_revision: client.committed_revision,
                sequence: client.sequence, changed: client.changed, duplicate: true });
        }
        if request.sequence < client.sequence { return Err(Error::Superseded); }
        if Some(request.sequence) != client.sequence.checked_add(1) { return Err(Error::OutOfOrder); }
        if !self.state.accepts_generation(request) { return Err(Error::StaleGeneration); }
        let mut proposed = self.state.clone(); proposed.advance()?;
        let preferences = WordPreferences::new(); preferences.restore(&proposed.entries);
        let changed = match &request.operation {
            Operation::Learn { raw, keys, korean, context } => preferences.remember(raw, keys, *korean, context),
            Operation::Forget { raw, keys } => {
                proposed.fence(Some(word_fingerprint(raw, keys)?))?;
                preferences.forget(raw, keys) != 0
            },
            Operation::Reset {} => {
                proposed.fence(None)?;
                let changed = preferences.count() != 0; preferences.clear(); changed
            },
        };
        proposed.entries = preferences.export().0;
        proposed.clients[index] = Client { id: client.id.clone(), lease_revision: client.lease_revision,
            sequence: request.sequence, fingerprint: Some(fingerprint), committed_revision: proposed.version.revision, changed,
            lifetime: client.lifetime };
        self.commit(proposed)?;
        Ok(Reply { version: self.state.version.clone(), committed_revision: self.state.version.revision,
            sequence: request.sequence, changed, duplicate: false })
    }
    fn checkpoint(&self, stage: Stage) -> Result<()> {
        #[cfg(test)] if let Some((injected, exit)) = self.injection {
            if stage == injected { if exit { std::process::exit(91); } return Err(Error::Io); }
        }
        let _ = stage; Ok(())
    }
    fn commit(&mut self, proposed: State) -> Result<()> {
        let result = (|| {
            let blob = encode_state(&proposed)?;
            let current = read_blob(&self.path)?.as_ref().map(|data| hex(&Sha256::digest(data)));
            if current != self.disk_hash { return Err(Error::ExternalChange); }
            self.checkpoint(Stage::BeforeWrite)?;
            let (temporary, mut file) = (0..16).find_map(|_| {
                // fetch_update supports the declared Rust 1.89 minimum (try_update is newer).
                #[allow(deprecated)]
                let counter = NEXT_TEMPORARY.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |v| v.checked_add(1)).ok()?;
                let temporary = suffix(&self.path, &format!(".pending-{}-{}-{}-{counter}", proposed.version.store_id, proposed.version.revision, std::process::id()));
                match OpenOptions::new().write(true).create_new(true).open(&temporary) {
                    Ok(file) => Some(Ok((temporary, file))), Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => None,
                    Err(_) => Some(Err(Error::Io)),
                }
            }).ok_or(Error::Exhausted)??;
            file.write_all(&blob).map_err(|_| Error::Io)?; file.sync_all().map_err(|_| Error::Io)?; drop(file);
            self.checkpoint(Stage::AfterSync)?;
            for attempt in 0..4 {
                let current = read_blob(&self.path)?.as_ref().map(|data| hex(&Sha256::digest(data)));
                if current != self.disk_hash { return Err(Error::ExternalChange); }
                match fs::rename(&temporary, &self.path) {
                    Ok(()) => break,
                    Err(e) if cfg!(windows) && matches!(e.raw_os_error(), Some(5 | 32)) && attempt < 3 => retry_io([10, 20, 40][attempt]),
                    Err(_) => return Err(Error::Io),
                }
            }
            self.checkpoint(Stage::AfterReplace)?;
            self.disk_hash = Some(hex(&Sha256::digest(&blob))); self.state = proposed; Ok(())
        })();
        // Only the committed name is authoritative. Reopen removes generated pending files before accepting actions.
        if result.is_err() { self.faulted = true; } result
    }
}
impl Request {
    pub fn decode(bytes: &[u8]) -> Result<Self> {
        if bytes.len() > MAX_REQUEST_BYTES { return Err(Error::InvalidRequest); }
        let request: Self = serde_json::from_slice(bytes).map_err(|_| Error::InvalidRequest)?;
        request.validate()?; Ok(request)
    }
    fn validate(&self) -> Result<()> {
        if !valid_id(&self.store_id) || !valid_id(&self.client_id) || self.lease_revision == 0 || self.sequence == 0 || self.generation == 0 {
            return Err(Error::InvalidRequest);
        }
        let word = |raw: &str, keys: &str| raw.len() <= 256 && keys.len() <= 256 && crate::prefs::normalize(raw, keys).is_some();
        let valid = match &self.operation {
            Operation::Learn { raw, keys, context, .. } => word(raw, keys) && crate::prefs::valid_context(context),
            Operation::Forget { raw, keys } => word(raw, keys), Operation::Reset {} => true,
        };
        if valid { Ok(()) } else { Err(Error::InvalidRequest) }
    }
    fn fingerprint(&self) -> String {
        let mut hash = Sha256::new(); hash.update(b"hanautomata-learning-request-v1");
        hash.update(self.store_id.as_bytes()); hash.update(self.client_id.as_bytes());
        hash.update(self.lease_revision.to_le_bytes()); hash.update(self.sequence.to_le_bytes()); hash.update(self.generation.to_le_bytes());
        let field = |hash: &mut Sha256, value: &str| { hash.update((value.len() as u32).to_le_bytes()); hash.update(value.as_bytes()); };
        match &self.operation {
            Operation::Learn { raw, keys, korean, context } => {
                hash.update([1, *korean as u8]); field(&mut hash, raw); field(&mut hash, keys); field(&mut hash, context);
            },
            Operation::Forget { raw, keys } => { hash.update([2]); field(&mut hash, raw); field(&mut hash, keys); },
            Operation::Reset {} => hash.update([3]),
        }
        hex(&hash.finalize())
    }
}

#[cfg(test)]
#[path = "learning_tests.rs"]
mod tests;
