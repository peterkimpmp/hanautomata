use super::*;
#[test]
#[ignore = "owned parent-exit fixture; the controlling test terminates only this child"]
fn parent_wait_fixture() { std::thread::sleep(Duration::from_secs(30)); }

#[test]
fn parent_exit_cancels_and_drains_a_long_accept_and_the_pipe_can_be_reused() {
    let child = crate::learning_process::Child::spawn(&std::env::current_exe().unwrap(),
        &["--ignored".into(), "--exact".into(), "learning_pipe::tests::parent_wait_fixture".into()]).unwrap();
    let parent = child.peer().unwrap();
    let endpoint = Endpoint::random().unwrap(); let mut listener = Listener::bind(&endpoint).unwrap();
    let raw = HANDLE(listener.pipe.as_raw_handle());
    let killer = std::thread::spawn(move || { std::thread::sleep(Duration::from_millis(50)); drop(child); });
    let before = Instant::now();
    let result = io_until_peer_exit(raw, deadline(Duration::from_secs(30)).unwrap(), Some(parent.handle()),
        |overlapped| unsafe { ConnectNamedPipe(raw, Some(overlapped)) });
    killer.join().unwrap(); assert_eq!(result, Err(Error::Cancelled)); assert!(before.elapsed() < Duration::from_secs(3));
    unsafe { let _ = DisconnectNamedPipe(raw); }
    let server = std::thread::spawn(move || listener.exchange(None, Duration::from_secs(2), |_, _| json!({"ok":true})));
    let this = Peer::process(std::process::id()).unwrap();
    let reply = call(&endpoint, &this, &Envelope::new("11111111111111111111111111111111", Command::Snapshot {}).unwrap(), Duration::from_secs(2));
    assert_eq!(reply.unwrap()["ok"], true); assert!(server.join().unwrap().is_ok());
}

#[test]
fn idle_product_companion_keeps_one_pending_accept_and_wakes_for_close() {
    use std::sync::{Arc, atomic::{AtomicUsize, Ordering}};
    let endpoint = Endpoint::random().unwrap(); let nonce = Endpoint::random().unwrap();
    let parent = Peer::process(std::process::id()).unwrap();
    let attempts = Arc::new(AtomicUsize::new(0)); let observed = attempts.clone();
    let server_endpoint = endpoint.clone(); let server_nonce = nonce.token().to_owned(); let server_parent = parent.clone();
    let unused_path = std::env::temp_dir().join(format!("hanautomata-no-store-{}", endpoint.token()));
    let path = unused_path.clone();
    let server = std::thread::spawn(move || {
        ACCEPT_OBSERVER.with(|observer| *observer.borrow_mut() = Some(observed));
        companion(server_endpoint, server_parent, server_nonce, path)
    });
    let limit = Instant::now() + Duration::from_secs(3);
    while attempts.load(Ordering::Relaxed) == 0 {
        assert!(Instant::now() < limit); std::thread::sleep(Duration::from_millis(5));
    }
    std::thread::sleep(Duration::from_millis(1200));
    let idle_attempts = attempts.load(Ordering::Relaxed);
    let before_close = Instant::now();
    let response = call(&endpoint, &parent, &Envelope::new(nonce.token(), Command::Close { lease_revision: 0 }).unwrap(), Duration::from_secs(2));
    assert!(server.join().unwrap().is_ok()); assert_eq!(response.unwrap()["ok"], true);
    assert!(!unused_path.exists(), "closing before Register must not create a store");
    assert!(before_close.elapsed() < Duration::from_secs(2));
    assert_eq!(idle_attempts, 1, "an idle companion must retain its pending accept instead of polling");
}

#[test]
fn slow_store_work_does_not_consume_the_response_transport_budget() {
    let endpoint = Endpoint::random().unwrap(); let mut listener = Listener::bind(&endpoint).unwrap();
    let peer = Peer::process(std::process::id()).unwrap();
    let server = std::thread::spawn(move || listener.exchange(None, Duration::from_millis(500), |_, _| {
        // Inject storage latency after the authenticated request, before serializing the response.
        std::thread::sleep(Duration::from_millis(800)); json!({"ok":true})
    }));
    let result = call(&endpoint, &peer, &Envelope::new("11111111111111111111111111111111", Command::Register {}).unwrap(), Duration::from_secs(3));
    let served = server.join().unwrap();
    assert!(served.is_ok(), "completed storage work needs a fresh reply transport budget: {served:?}");
    assert_eq!(result.unwrap()["ok"], true);
}
#[test]
fn endpoints_cannot_select_remote_or_installed_pipes() {
    assert!(Endpoint::random().is_ok());
    for bad in ["", "0", "00000000000000000000000000000000", "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA", "..\\live", "\\\\server\\pipe\\x"] {
        assert!(Endpoint::from_token(bad).is_err());
    }
}
#[test]
fn wire_contract_refuses_claimed_identity_and_unknown_commands() {
    let good = serde_json::to_value(Envelope::new("11111111111111111111111111111111", Command::Register {}).unwrap()).unwrap();
    for key in ["pid", "sid", "client_id", "store_path", "session", "lease_revision"] {
        let mut bad = good.clone(); bad[key] = json!("claimed"); assert!(serde_json::from_value::<Envelope>(bad).is_err());
    }
    for command in ["resume", "replace", "retire", "clients", "reconcile"] {
        let mut bad = good.clone(); bad["command"]["command"] = json!(command); assert!(serde_json::from_value::<Envelope>(bad).is_err());
    }
}
#[test]
fn requested_client_rights_cannot_create_an_extra_server() {
    use windows::Win32::Storage::FileSystem::{FILE_APPEND_DATA, FILE_GENERIC_READ, FILE_WRITE_DATA};
    assert_eq!(CLIENT_ACCESS, FILE_GENERIC_READ.0 | FILE_WRITE_DATA.0);
    assert_eq!(CLIENT_ACCESS & (FILE_APPEND_DATA.0 | 0x0004_0000 | 0x0008_0000), 0);
}
#[test]
fn stalled_accept_is_cancelled_and_listener_remains_owned() {
    let endpoint = Endpoint::random().unwrap(); let listener = Listener::bind(&endpoint).unwrap();
    let raw = HANDLE(listener.pipe.as_raw_handle()); let before = Instant::now();
    assert_eq!(io(raw, deadline(Duration::from_millis(35)).unwrap(), |overlapped| unsafe { ConnectNamedPipe(raw, Some(overlapped)) }), Err(Error::Timeout));
    assert!(before.elapsed() < Duration::from_secs(2));
    assert!(Listener::bind(&endpoint).is_err(), "a second listener must not steal a live endpoint");
    drop(listener); assert!(Listener::bind(&endpoint).is_ok());
}

fn raw_client(endpoint: &Endpoint, qos: windows::Win32::Storage::FileSystem::FILE_FLAGS_AND_ATTRIBUTES) -> OwnedHandle {
    owned(unsafe { CreateFileW(&endpoint.name(), CLIENT_ACCESS, FILE_SHARE_MODE(0), None, OPEN_EXISTING,
        FILE_FLAG_OVERLAPPED | SECURITY_SQOS_PRESENT | qos, None) }.unwrap())
}

#[test]
fn partial_and_oversized_frames_have_a_fixed_deadline_and_allocation_bound() {
    for (bytes, expected) in [
        (vec![0, 0, 0, 0], Error::Protocol),
        (((MAX_COMMAND + 1) as u32).to_le_bytes().to_vec(), Error::Protocol),
        (vec![10, 0], Error::Timeout),
        (vec![10, 0, 0, 0, b'{'], Error::Timeout),
    ] {
        let endpoint = Endpoint::random().unwrap(); let listener = Listener::bind(&endpoint).unwrap();
        let server = std::thread::spawn(move || {
            let raw = HANDLE(listener.pipe.as_raw_handle()); let end = deadline(Duration::from_millis(200)).unwrap();
            io(raw, end, |overlapped| unsafe { ConnectNamedPipe(raw, Some(overlapped)) }).unwrap();
            read_packet(raw, MAX_COMMAND, end)
        });
        let client = raw_client(&endpoint, SECURITY_IDENTIFICATION);
        write_all(HANDLE(client.as_raw_handle()), &bytes, deadline(Duration::from_secs(1)).unwrap()).unwrap();
        assert_eq!(server.join().unwrap(), Err(expected)); drop(client);
        assert!(Listener::bind(&endpoint).is_ok(), "cancelled I/O must release its endpoint");
    }
}

#[test]
fn anonymous_identity_is_refused_and_impersonation_is_always_reverted() {
    use windows::Win32::Storage::FileSystem::SECURITY_ANONYMOUS;
    for (qos, accepted) in [(SECURITY_IDENTIFICATION, true), (SECURITY_ANONYMOUS, false)] {
        let endpoint = Endpoint::random().unwrap(); let listener = Listener::bind(&endpoint).unwrap();
        let server = std::thread::spawn(move || {
            let raw = HANDLE(listener.pipe.as_raw_handle()); let end = deadline(Duration::from_secs(2)).unwrap();
            io(raw, end, |overlapped| unsafe { ConnectNamedPipe(raw, Some(overlapped)) }).unwrap();
            let peer = Peer::connected_client(raw, &listener.owner).unwrap();
            assert_eq!(read_packet(raw, MAX_COMMAND, end).unwrap(), b"identity-only");
            let result = peer.check_client_message(raw);
            assert!(identity::no_thread_token().is_ok(), "no request may leave the worker impersonating a client");
            result.is_ok()
        });
        let client = raw_client(&endpoint, qos);
        write_packet(HANDLE(client.as_raw_handle()), b"identity-only", MAX_COMMAND, deadline(Duration::from_secs(1)).unwrap()).unwrap();
        assert_eq!(server.join().unwrap(), accepted);
    }
}

#[test]
fn stalled_reply_write_is_cancelled_without_freeing_an_active_buffer() {
    let endpoint = Endpoint::random().unwrap(); let listener = Listener::bind(&endpoint).unwrap();
    let server = std::thread::spawn(move || {
        let raw = HANDLE(listener.pipe.as_raw_handle()); let end = deadline(Duration::from_millis(200)).unwrap();
        io(raw, end, |overlapped| unsafe { ConnectNamedPipe(raw, Some(overlapped)) }).unwrap();
        let payload = vec![b'x'; MAX_REPLY]; write_packet(raw, &payload, MAX_REPLY, end)
    });
    let _client = raw_client(&endpoint, SECURITY_IDENTIFICATION); // deliberately never reads
    assert_eq!(server.join().unwrap(), Err(Error::Timeout));
}
