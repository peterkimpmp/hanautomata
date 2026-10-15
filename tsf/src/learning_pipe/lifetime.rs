//! Cross-session, kernel-owned lease liveness. Only explicitly managed leases may be reclaimed.
//! All inspection/registration happens under Authority's file lock. Uncertainty always preserves a lease.
use super::{owned, LocalMemory, Peer};
use hanautomata_core::learning::{Authority, Error, LeaseLifetime};
use std::{marker::PhantomData, mem::size_of, os::windows::io::{AsRawHandle, OwnedHandle}, rc::Rc};
use windows::{core::HSTRING, Win32::{
    Foundation::{HANDLE, HLOCAL, ERROR_FILE_NOT_FOUND, WAIT_OBJECT_0, WAIT_ABANDONED},
    Security::{Authorization::{ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1}, PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES},
    System::Threading::{CreateMutexExW, OpenMutexW, ReleaseMutex, WaitForSingleObject, MUTEX_MODIFY_STATE, SYNCHRONIZATION_SYNCHRONIZE},
}};

// Ownership belongs to the creating thread. Never move this guard to another worker.
pub(super) struct Guard { handle: OwnedHandle, _thread: PhantomData<Rc<()>> }
impl Drop for Guard { fn drop(&mut self) { unsafe { let _ = ReleaseMutex(HANDLE(self.handle.as_raw_handle())); } } }
fn name(store: &str, client: &str) -> Result<HSTRING, Error> {
    if !super::valid_token(store) || !super::valid_token(client) { return Err(Error::InvalidRequest); }
    Ok(HSTRING::from(format!(r"Global\Hanautomata.LearningLease.1.{store}.{client}")))
}
fn take(handle: OwnedHandle) -> Option<Guard> {
    match unsafe { WaitForSingleObject(HANDLE(handle.as_raw_handle()), 0) } {
        WAIT_OBJECT_0 | WAIT_ABANDONED => Some(Guard { handle, _thread: PhantomData }),
        _ => None,
    }
}
pub(super) fn acquire(store: &str, client: &str, owner: &Peer) -> Result<Guard, Error> {
    // A user SID (not one logon SID) permits the same user's other sessions to inspect liveness.
    // No World, AppContainer or integrity-policy changes. Global mutex creation needs no global mapping privilege.
    let sddl = HSTRING::from(format!("D:P(A;;0x100001;;;{})", owner.user_sid()));
    let mut descriptor = PSECURITY_DESCRIPTOR::default();
    unsafe { ConvertStringSecurityDescriptorToSecurityDescriptorW(&sddl, SDDL_REVISION_1, &mut descriptor, None) }.map_err(|_| Error::Io)?;
    let _descriptor = LocalMemory(HLOCAL(descriptor.0));
    let attributes = SECURITY_ATTRIBUTES { nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: descriptor.0, bInheritHandle: false.into() };
    let handle = owned(unsafe { CreateMutexExW(Some(&attributes), &name(store, client)?, 0,
        (MUTEX_MODIFY_STATE | SYNCHRONIZATION_SYNCHRONIZE).0) }.map_err(|_| Error::Io)?);
    take(handle).ok_or(Error::Busy)
}
enum Observation { Dead(Option<Guard>), Preserve }
fn observe(store: &str, client: &str) -> Result<Observation, Error> {
    match unsafe { OpenMutexW(MUTEX_MODIFY_STATE | SYNCHRONIZATION_SYNCHRONIZE, false, &name(store, client)?) } {
        Ok(handle) => Ok(match take(owned(handle)) { Some(guard) => Observation::Dead(Some(guard)), None => Observation::Preserve }),
        Err(error) if error.code() == ERROR_FILE_NOT_FOUND.to_hresult() => Ok(Observation::Dead(None)),
        Err(_) => Ok(Observation::Preserve), // denied, wrong object type, or unknown error is never proof of death
    }
}
pub(super) fn reclaim(owner: &mut Authority, store: &str, own_client: &str) -> Result<usize, Error> {
    retire_dead(owner, store, Some(own_client), false)
}
pub(super) fn prepare_rollback(owner: &mut Authority, store: &str) -> Result<usize, Error> {
    retire_dead(owner, store, None, true)
}
fn retire_dead(owner: &mut Authority, store: &str, own_client: Option<&str>, require_drained: bool) -> Result<usize, Error> {
    let mut keep = vec![]; let mut dead_guards = vec![];
    for client in owner.clients()? {
        if client.lifetime != LeaseLifetime::WindowsMutexV1 || Some(client.key.client_id.as_str()) == own_client {
            keep.push(client.key); continue;
        }
        match observe(store, &client.key.client_id)? {
            Observation::Dead(guard) => dead_guards.push(guard),
            Observation::Preserve if require_drained => return Err(Error::Busy),
            Observation::Preserve => keep.push(client.key),
        }
    }
    // Keep acquired abandoned/unowned mutexes until the transaction is committed, preventing a reclaim race.
    let removed = owner.retire_absent(&keep)?; drop(dead_guards); Ok(removed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::learning_pipe::Endpoint;
    use std::{sync::mpsc, thread};
    use windows::Win32::System::Threading::CreateEventW;
    struct StoreFiles(std::path::PathBuf);
    impl StoreFiles {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!("hanautomata-rollback-{}", Endpoint::random().unwrap().token()));
            std::fs::create_dir(&path).unwrap(); Self(path)
        }
        fn path(&self) -> std::path::PathBuf { self.0.join("learning.db") }
    }
    impl Drop for StoreFiles { fn drop(&mut self) {
        for name in ["learning.db", "learning.db.owner.lock"] { let _ = std::fs::remove_file(self.0.join(name)); }
        let _ = std::fs::remove_dir(&self.0);
    } }
    #[test]
    fn rollback_preserves_live_leases_and_data_then_drains_only_dead_managed_leases() {
        let files = StoreFiles::new(); let store = Endpoint::random().unwrap().token().to_owned();
        let live = Endpoint::random().unwrap().token().to_owned(); let dead = Endpoint::random().unwrap();
        let unmanaged = Endpoint::random().unwrap();
        let (ready_tx, ready_rx) = mpsc::channel(); let (stop_tx, stop_rx) = mpsc::channel();
        let (s, c) = (store.clone(), live.clone());
        let worker = thread::spawn(move || {
            let peer = Peer::process(std::process::id()).unwrap(); let guard = acquire(&s, &c, &peer).unwrap();
            ready_tx.send(()).unwrap(); stop_rx.recv().unwrap(); drop(guard);
        });
        ready_rx.recv().unwrap();
        let mut owner = Authority::open(files.path(), &store).unwrap();
        let lease = owner.register_with_lifetime(&live, LeaseLifetime::WindowsMutexV1).unwrap();
        owner.register_with_lifetime(dead.token(), LeaseLifetime::WindowsMutexV1).unwrap();
        owner.register(unmanaged.token()).unwrap();
        owner.apply(&hanautomata_core::learning::Request { store_id: store.clone(), client_id: live,
            lease_revision: lease.lease_revision, sequence: 1, generation: lease.version.generation,
            operation: hanautomata_core::learning::Operation::Learn { raw: "retained".into(), keys: "retained".into(), korean: true, context: String::new() } }).unwrap();
        let before = std::fs::read(files.path()).unwrap(); let words = owner.snapshot().unwrap().entries;
        let blocked = prepare_rollback(&mut owner, &store);
        stop_tx.send(()).unwrap(); worker.join().unwrap();
        assert_eq!(blocked, Err(Error::Busy)); assert_eq!(std::fs::read(files.path()).unwrap(), before);
        assert_eq!(prepare_rollback(&mut owner, &store).unwrap(), 2);
        assert_eq!(owner.snapshot().unwrap().entries, words);
        assert_eq!(owner.clients().unwrap().len(), 1); assert_eq!(owner.clients().unwrap()[0].key.client_id, unmanaged.token());
        assert!(std::fs::read(files.path()).unwrap().starts_with(b"Hanautomata.TsfLearning/1\n"));
        drop(owner);
    }
    #[test]
    fn rollback_refuses_ambiguous_liveness_without_writing() {
        let files = StoreFiles::new(); let store = Endpoint::random().unwrap(); let client = Endpoint::random().unwrap();
        let collision = owned(unsafe { CreateEventW(None, true, false, &name(store.token(), client.token()).unwrap()).unwrap() });
        let mut owner = Authority::open(files.path(), store.token()).unwrap();
        owner.register_with_lifetime(client.token(), LeaseLifetime::WindowsMutexV1).unwrap();
        let before = std::fs::read(files.path()).unwrap();
        assert_eq!(prepare_rollback(&mut owner, store.token()), Err(Error::Busy));
        assert_eq!(std::fs::read(files.path()).unwrap(), before); drop(owner); drop(collision);
    }
    #[test]
    fn liveness_observation_preserves_owned_and_wrong_type_objects_and_detects_owner_exit() {
        let store = Endpoint::random().unwrap().token().to_owned(); let client = Endpoint::random().unwrap().token().to_owned();
        assert!(matches!(observe(&store, &client).unwrap(), Observation::Dead(None)));
        let (ready_tx, ready_rx) = mpsc::channel(); let (stop_tx, stop_rx) = mpsc::channel();
        let (s, c) = (store.clone(), client.clone());
        let worker = thread::spawn(move || {
            let peer = Peer::process(std::process::id()).unwrap(); let guard = acquire(&s, &c, &peer).unwrap();
            ready_tx.send(()).unwrap(); stop_rx.recv().unwrap(); drop(guard);
        });
        ready_rx.recv().unwrap(); assert!(matches!(observe(&store, &client).unwrap(), Observation::Preserve));
        stop_tx.send(()).unwrap(); worker.join().unwrap(); assert!(matches!(observe(&store, &client).unwrap(), Observation::Dead(None)));
        let collision = owned(unsafe { CreateEventW(None, true, false, &name(&store, &client).unwrap()).unwrap() });
        assert!(matches!(observe(&store, &client).unwrap(), Observation::Preserve)); drop(collision);
    }
}
