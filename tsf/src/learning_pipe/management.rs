//! Explicit desktop settings requests. Separate from the input endpoint; stdin never selects a path or identity.
use super::{identity, Peer};
use hanautomata_core::learning::{Authority, Error, Operation, Version};
use serde::Deserialize;
use serde_json::{json, Value};
use std::{io::Read, path::Path};
const MAX_MANAGEMENT_REQUEST: usize = 4096;
const UTF8_PREAMBLE: &[u8] = &[0xef, 0xbb, 0xbf];

#[derive(Deserialize)]
#[serde(tag = "command", rename_all = "snake_case", deny_unknown_fields)]
enum Command { Snapshot {}, Forget { version: Version, raw: String, keys: String }, Reset { version: Version } }

fn failure(error: impl serde::Serialize) -> Value { json!({"protocol":1,"ok":false,"error":error}) }
pub(crate) fn execute(path: &Path, input: &[u8]) -> Value {
    // A redirected .NET Framework writer can emit one UTF-8 preamble when the console code page is
    // 65001. It is transport metadata, not JSON; preserve the same 4096-byte JSON limit after it.
    let input = input.strip_prefix(UTF8_PREAMBLE).unwrap_or(input);
    let command = if input.len() <= MAX_MANAGEMENT_REQUEST { serde_json::from_slice::<Command>(input).ok() } else { None };
    let Some(command) = command else { return failure(Error::InvalidRequest); };
    if identity::no_thread_token().is_err() || !crate::system::desktop_settings_allowed()
        || Peer::process(std::process::id()).is_err() { return failure("unauthorized"); }
    if !path.is_absolute() { return failure(Error::InvalidRequest); }
    let result = (|| {
        let mut owner = Authority::open_existing(path)?;
        match command {
            Command::Snapshot {} => owner.snapshot(),
            Command::Forget { version, raw, keys } => owner.manage(&version, Operation::Forget { raw, keys }),
            Command::Reset { version } => owner.manage(&version, Operation::Reset {}),
        }
    })();
    match result { Ok(snapshot) => json!({"protocol":1,"ok":true,"snapshot":snapshot}), Err(error) => failure(error) }
}

pub fn management_main() -> i32 {
    let mut input = Vec::new();
    let result = if std::io::stdin().lock().take((MAX_MANAGEMENT_REQUEST + UTF8_PREAMBLE.len() + 1) as u64).read_to_end(&mut input).is_err() {
        failure(Error::Io)
    } else {
        match crate::system::settings_dir() {
            Ok(directory) => execute(&directory.join("tsf-learning.db"), &input),
            Err(_) => failure(Error::Io),
        }
    };
    // Only this explicit settings command returns words, through the caller's private stdout pipe. Never log them.
    println!("{result}"); if result["ok"] == true { 0 } else { 2 }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::Endpoint;
    use hanautomata_core::WordPreferences;
    struct Files(std::path::PathBuf);
    impl Drop for Files { fn drop(&mut self) {
        for name in ["learning.db", "learning.db.owner.lock"] { let _ = std::fs::remove_file(self.0.join(name)); }
        let _ = std::fs::remove_dir(&self.0);
    } }
    #[test]
    fn settings_protocol_refuses_path_identity_learning_and_oversize_without_creating_files() {
        let root = std::env::temp_dir().join(format!("hanautomata-management-{}", Endpoint::random().unwrap().token()));
        let files = Files(root); let path = files.0.join("learning.db");
        for input in [br#"{"command":"snapshot","path":"other.db"}"#.to_vec(),
            br#"{"command":"snapshot","pid":1}"#.to_vec(), br#"{"command":"learn"}"#.to_vec(),
            vec![b' '; MAX_MANAGEMENT_REQUEST + 1]] {
            let value = execute(&path, &input); assert_eq!(value["error"], "invalid_request");
        }
        assert_eq!(execute(&path, br#"{"command":"snapshot"}"#)["error"], "missing_state");
        let mut bom = vec![0xef, 0xbb, 0xbf]; bom.extend_from_slice(br#"{"command":"snapshot"}"#);
        assert_eq!(execute(&path, &bom)["error"], "missing_state", "UTF-8 console preamble is not part of JSON");
        bom.resize(MAX_MANAGEMENT_REQUEST + 3, b' ');
        assert_eq!(execute(&path, &bom)["error"], "missing_state");
        bom.push(b' '); assert_eq!(execute(&path, &bom)["error"], "invalid_request");
        assert_eq!(execute(&path, &[&[0xef, 0xbb, 0xbf][..], &bom[..26]].concat())["error"], "invalid_request");
        assert!(!files.0.exists());
    }
    #[test]
    fn settings_protocol_lists_then_deletes_exact_snapshot_and_rejects_stale_reset() {
        let root = std::env::temp_dir().join(format!("hanautomata-management-{}", Endpoint::random().unwrap().token()));
        let files = Files(root); let path = files.0.join("learning.db"); let prefs = WordPreferences::new();
        prefs.remember_global("hello", "hello", true);
        drop(Authority::open_seeded(&path, Endpoint::random().unwrap().token(), &prefs.export().0).unwrap());
        let snapshot = execute(&path, br#"{"command":"snapshot"}"#);
        assert_eq!(snapshot["ok"], true); assert_eq!(snapshot["snapshot"]["entries"].as_array().unwrap().len(), 1);
        let request = json!({"command":"forget","version":snapshot["snapshot"]["version"],"raw":"hello","keys":"hello"});
        let deleted = execute(&path, &serde_json::to_vec(&request).unwrap());
        assert_eq!(deleted["ok"], true); assert!(deleted["snapshot"]["entries"].as_array().unwrap().is_empty());
        let before = std::fs::read(&path).unwrap();
        let stale = json!({"command":"reset","version":snapshot["snapshot"]["version"]});
        assert_eq!(execute(&path, &serde_json::to_vec(&stale).unwrap())["error"], "stale_generation");
        assert_eq!(std::fs::read(&path).unwrap(), before);
    }
    #[test]
    fn native_snapshots_cover_seeded_contexts_boundaries_forget_and_reset() {
        use std::io::Write;
        let files = Files(std::env::temp_dir().join(format!("hanautomata-management-{}", Endpoint::random().unwrap().token())));
        let path = files.0.join("learning.db"); let prefs = WordPreferences::new();
        prefs.remember_global("hello", "hello", false);
        prefs.remember_global("dkssud", "dkssud", true);
        prefs.remember("hello", "hello", false, &hanautomata_core::context_key("한글"));
        prefs.remember_global(&"a".repeat(64), &"a".repeat(64), true);
        let mut entries = prefs.export().0;
        entries[0].korean = true; entries[0].korean_weight = 16.0; entries[0].english_weight = 0.0;
        drop(Authority::open_seeded(&path, Endpoint::random().unwrap().token(), &entries).unwrap());
        let before = execute(&path, br#"{"command":"snapshot"}"#);
        assert_eq!(before["ok"], true); assert_eq!(before["snapshot"]["entries"].as_array().unwrap().len(), 4);
        let deleted = execute(&path, &serde_json::to_vec(&json!({"command":"forget","version":before["snapshot"]["version"],"raw":"hello","keys":"hello"})).unwrap());
        assert_eq!(deleted["ok"], true); assert_eq!(deleted["snapshot"]["entries"].as_array().unwrap().len(), 2);
        let reset = execute(&path, &serde_json::to_vec(&json!({"command":"reset","version":deleted["snapshot"]["version"]})).unwrap());
        assert_eq!(reset["ok"], true); assert!(reset["snapshot"]["entries"].as_array().unwrap().is_empty());
        let maximum = Files(std::env::temp_dir().join(format!("hanautomata-management-{}", Endpoint::random().unwrap().token())));
        let maximum_path = maximum.0.join("learning.db"); let full = WordPreferences::new();
        assert_eq!(hanautomata_core::CAPACITY, 1024, "shared with the C# response contract");
        for n in 0..hanautomata_core::CAPACITY {
            let word = format!("Ab{}{}{}", (b'a' + (n / 676) as u8) as char, (b'a' + (n / 26 % 26) as u8) as char, (b'a' + (n % 26) as u8) as char);
            assert!(full.remember_global(&word, &word, n % 2 == 0));
        }
        drop(Authority::open_seeded(&maximum_path, Endpoint::random().unwrap().token(), &full.export().0).unwrap());
        let capacity = execute(&maximum_path, br#"{"command":"snapshot"}"#);
        assert_eq!(capacity["ok"], true); assert_eq!(capacity["snapshot"]["entries"].as_array().unwrap().len(), hanautomata_core::CAPACITY);
        // Explicit export by this test executable only, never compiled into the product command. The C#
        // consumer reads these exact native protocol bytes, not a separately maintained JSON example.
        if let Some(output) = std::env::var_os("HANAUTOMATA_MANAGEMENT_FIXTURE") {
            let mut output = std::fs::OpenOptions::new().write(true).create_new(true).open(output).unwrap();
            for snapshot in [before, deleted, reset, capacity] { writeln!(output, "{snapshot}").unwrap(); }
        }
    }
}
