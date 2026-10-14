use super::*;
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
