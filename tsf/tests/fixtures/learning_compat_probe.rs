//! Used only by test-learning-rollback.ps1, against an archived old core and the current core.
use hanautomata_core::learning::{Authority, Operation, Request};
const STORE: &str = "11111111111111111111111111111111";
const OLD: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
#[cfg(feature = "managed")]
const MANAGED: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
fn apply(owner: &mut Authority, operation: Operation) -> Result<(), Box<dyn std::error::Error>> {
    let lease = owner.register(OLD)?;
    owner.apply(&Request { store_id: STORE.into(), client_id: OLD.into(), lease_revision: lease.lease_revision,
        sequence: lease.next_sequence, generation: lease.version.generation, operation })?; Ok(())
}
fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args_os().collect();
    if args.len() != 3 { return Err("expected action and isolated store".into()); }
    let mut owner = Authority::open(std::path::Path::new(&args[2]), STORE)?;
    let action = args[1].to_str().ok_or("invalid action")?;
    match action {
        "read" => {},
        "before" | "after" => {
            apply(&mut owner, Operation::Learn { raw: action.into(), keys: action.into(), korean: true, context: String::new() })?;
        },
        "edges" => {
            // Stored raw/keys are Latin keystrokes; the preceding-word fingerprint may come from Hangul.
            apply(&mut owner, Operation::Learn { raw: "dkssud".into(), keys: "dkssud".into(), korean: true,
                context: hanautomata_core::context_key("한글") })?;
            apply(&mut owner, Operation::Learn { raw: "temporary".into(), keys: "temporary".into(), korean: false, context: String::new() })?;
            apply(&mut owner, Operation::Forget { raw: "temporary".into(), keys: "temporary".into() })?;
        },
        "reset" => { apply(&mut owner, Operation::Reset {})?; },
        "boundary" => {
            // Fixture-only encoding of valid extreme weights, beyond the decay model's natural steady state.
            // No live owner may hold a cached hash during this deliberate, checksummed file mutation.
            use sha2::{Digest, Sha256};
            drop(owner);
            let bytes = std::fs::read(&args[2])?;
            let newline = bytes.iter().position(|b| *b == b'\n').ok_or("missing header")?;
            let mut body: serde_json::Value = serde_json::from_slice(&bytes[newline + 66..])?;
            body["entries"][0]["korean_weight"] = serde_json::json!(16.0);
            body["entries"][0]["english_weight"] = serde_json::json!(0.0);
            body["entries"][1]["korean_weight"] = serde_json::json!(0.0);
            body["entries"][1]["english_weight"] = serde_json::json!(16.0);
            let body = serde_json::to_vec(&body)?;
            let digest: String = Sha256::digest(&body).iter().map(|b| format!("{b:02x}")).collect();
            let mut updated = bytes[..=newline].to_vec(); updated.extend_from_slice(digest.as_bytes());
            updated.push(b'\n'); updated.extend_from_slice(&body); std::fs::write(&args[2], updated)?;
            owner = Authority::open(std::path::Path::new(&args[2]), STORE)?;
        },
        #[cfg(feature = "managed")]
        "managed" => {
            let lease = owner.register_with_lifetime(MANAGED, hanautomata_core::learning::LeaseLifetime::WindowsMutexV1)?;
            owner.apply(&Request { store_id: STORE.into(), client_id: MANAGED.into(), lease_revision: lease.lease_revision,
                sequence: lease.next_sequence, generation: lease.version.generation,
                operation: Operation::Learn { raw: "retained".into(), keys: "retained".into(), korean: true, context: String::new() } })?;
        },
        #[cfg(feature = "managed")]
        "drain" => {
            // Synthetic lease owned by this fixture, not a product liveness decision.
            let lease = owner.clients()?.into_iter().find(|client| client.key.client_id == MANAGED).ok_or("missing fixture lease")?;
            owner.retire(MANAGED, lease.key.lease_revision)?;
        },
        _ => return Err("unsupported action".into()),
    }
    println!("{}", serde_json::to_string(&owner.snapshot()?)?); Ok(())
}
fn main() {
    if let Err(error) = run() { eprintln!("{error}"); std::process::exit(3); }
}
