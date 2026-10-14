//! User settings for the Windows TSF backend. Loaded at activation, never on the key callback path.
use serde::{Deserialize, Serialize};
use std::{fs::File, io::{self, Read}, path::Path};
use hanautomata_ffi::{InputMode, KeyboardKind, SwitchKind};
use crate::shell::{Settings as Keys, Shell};

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(default, rename_all = "PascalCase", deny_unknown_fields)]
pub struct Settings {
    pub format: u32,
    pub input_mode: String,
    pub keyboard: String,
    pub switch_kind: String,
    pub sensitivity: String,
    pub semicolon_shortcut: bool,
    pub learn_choices: bool,
    pub learning_paused: bool,
    pub delete_syllable: bool,
    pub ms_clusters: bool,
    pub strict: bool,
    pub shift_space: bool,
    pub caps_lock: bool,
    pub hangul_key: bool,
    pub right_alt: bool,
    pub pause_key: bool,
    pub correction_keys: bool,
    pub excluded_apps: Vec<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Self { format: 1, input_mode: "Auto".into(), keyboard: "Dubeolsik".into(), switch_kind: "SingleShot".into(),
            sensitivity: "balanced".into(), semicolon_shortcut: true, learn_choices: true, learning_paused: false,
            delete_syllable: false, ms_clusters: false, strict: false, shift_space: true, caps_lock: true,
            hangul_key: true, right_alt: true, pause_key: true, correction_keys: true, excluded_apps: vec![] }
    }
}

impl Settings {
    pub const MAX_BYTES: usize = 65536;

    pub fn parse(bytes: &[u8]) -> Result<Self, &'static str> {
        if bytes.len() > Self::MAX_BYTES { return Err("settings exceeds 64 KiB"); }
        let bytes = bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(bytes);
        // Serde structs also accept positional arrays; the shared editor contract requires an object.
        if bytes.iter().find(|b| !b.is_ascii_whitespace()) != Some(&b'{') { return Err("settings must be a JSON object"); }
        let value: Self = serde_json::from_slice(bytes).map_err(|_| "invalid settings JSON or field type")?;
        value.validate()?;
        Ok(value)
    }

    pub fn validate(&self) -> Result<(), &'static str> {
        if self.format != 1 { return Err("unsupported settings format"); }
        if !matches!(self.input_mode.as_str(), "Auto" | "Korean" | "English") { return Err("invalid input mode"); }
        if !matches!(self.keyboard.as_str(), "Dubeolsik" | "Sebeolsik390" | "SebeolsikFinal") { return Err("invalid keyboard"); }
        if !matches!(self.switch_kind.as_str(), "SingleShot" | "Cycle" | "Toggle") { return Err("invalid switch kind"); }
        if !matches!(self.sensitivity.as_str(), "conservative" | "balanced" | "aggressive") { return Err("invalid sensitivity"); }
        if self.excluded_apps.len() > 128 { return Err("too many excluded apps"); }
        for app in &self.excluded_apps {
            if app.len() > 1020 || app.chars().count() > 255 || app.len() <= 4 || app.trim() != app
                || !app.to_ascii_lowercase().ends_with(".exe")
                || app.chars().any(|c| c.is_control() || "\\/:*?\"<>|".contains(c)) {
                return Err("excluded apps must be executable file names");
            }
        }
        Ok(())
    }

    /// No creation or repair here: unreadable/corrupt files remain available for explicit repair in the editor.
    pub fn load(path: &Path) -> Result<Self, &'static str> {
        let file = match File::open(path) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                // A writer leaves its coordination file. Missing data after a write began (including a partial
                // ReplaceFile failure) is a repair state, not first run. Do not silently reenable excluded apps.
                for suffix in [".lock", ".previous"] {
                    let mut marker = path.as_os_str().to_owned(); marker.push(suffix);
                    match std::fs::metadata(marker) {
                        Err(error) if error.kind() == io::ErrorKind::NotFound => {},
                        _ => return Err("settings missing after a previous write; repair required"),
                    }
                }
                return Ok(Self::default());
            }
            Err(_) => return Err("settings file cannot be opened"),
        };
        if !file.metadata().map_err(|_| "settings metadata unavailable")?.is_file() {
            return Err("settings path is not a file");
        }
        let mut bytes = Vec::new();
        file.take((Self::MAX_BYTES + 1) as u64).read_to_end(&mut bytes).map_err(|_| "settings file cannot be read")?;
        Self::parse(&bytes)
    }

    pub fn excludes(&self, process: &str) -> bool {
        #[cfg(windows)]
        {
            use windows::Win32::Globalization::{CompareStringOrdinal, CSTR_EQUAL};
            let process: Vec<u16> = process.encode_utf16().collect();
            self.excluded_apps.iter().any(|app| {
                let app: Vec<u16> = app.encode_utf16().collect();
                unsafe { CompareStringOrdinal(&app, &process, true) == CSTR_EQUAL }
            })
        }
        // The portable engine tests do not emulate the Windows filesystem's Unicode case table.
        #[cfg(not(windows))]
        self.excluded_apps.iter().any(|app| app.eq_ignore_ascii_case(process))
    }

    /// Only call on a fresh shell, before key handling begins; setters may commit an existing composition.
    pub fn apply_new_session(&self, shell: &mut Shell) {
        let engine = shell.engine();
        engine.set_mode(match self.input_mode.as_str() {
            "Korean" => InputMode::Korean, "English" => InputMode::English, _ => InputMode::Auto,
        });
        engine.set_keyboard(match self.keyboard.as_str() {
            "Sebeolsik390" => KeyboardKind::Sebeolsik390, "SebeolsikFinal" => KeyboardKind::SebeolsikFinal, _ => KeyboardKind::Dubeolsik,
        });
        engine.set_switch_kind(match self.switch_kind.as_str() {
            "Cycle" => SwitchKind::Cycle, "Toggle" => SwitchKind::Toggle, _ => SwitchKind::SingleShot,
        });
        engine.set_sensitivity(self.sensitivity.clone());
        engine.set_semicolon_shortcut(self.semicolon_shortcut);
        engine.set_learning_enabled(self.learn_choices);
        engine.set_learning_paused(self.learning_paused);
        engine.set_delete_syllable(self.delete_syllable);
        engine.set_ms_clusters(self.ms_clusters);
        engine.set_strict(self.strict);
        shell.settings = Keys { shift_space: self.shift_space, caps_lock: self.caps_lock,
            hangul_key: self.hangul_key, right_alt: self.right_alt, pause_key: self.pause_key,
            correction_keys: self.correction_keys };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use hanautomata_ffi::Engine;

    #[test]
    fn saved_options_apply_to_a_new_shell() {
        let settings = Settings::parse(br#"{"Format":1,"InputMode":"Korean","Keyboard":"Sebeolsik390","SwitchKind":"Cycle","Sensitivity":"conservative","SemicolonShortcut":false,"LearnChoices":false,"LearningPaused":true,"ShiftSpace":false,"RightAlt":false,"ExcludedApps":["code.exe"]}"#).unwrap();
        let mut shell = Shell::new(Engine::new(String::new(), std::env::temp_dir().join("hanautomata-settings-test-unused.json").to_string_lossy().into_owned()));
        settings.apply_new_session(&mut shell);
        assert_eq!(shell.mode(), InputMode::Korean);
        assert_eq!(shell.engine().keyboard(), KeyboardKind::Sebeolsik390);
        assert_eq!(shell.engine().switch_kind(), SwitchKind::Cycle);
        assert_eq!(shell.engine().sensitivity(), "conservative");
        assert!(!shell.engine().learning_enabled());
        assert!(shell.engine().learning_paused());
        assert!(!shell.settings.shift_space);
        assert!(!shell.settings.right_alt);
        assert!(settings.excludes("CODE.EXE"));
        assert!(!settings.excludes("decode.exe"));
        let _: &Arc<Engine> = shell.engine();
    }

    #[test]
    fn malformed_future_or_oversized_settings_are_not_silently_accepted() {
        for text in ["{", "null", "[]", "[1,\"Korean\"]", "{} {}", r#"{"Format":2}"#, r#"{"Keyboard":"unknown"}"#, r#"{"ShiftSpace":"false"}"#, r#"{"ExcludedApps":["C:\\apps\\code.exe"]}"#,
            r#"{"RightAltt":false}"#, r#"{"Format":1,"Format":2}"#, r#"{"ExcludedApps":null}"#, r#"{"InputMode":"auto"}"#,
            r#"{"SwitchKind":"invalid"}"#, r#"{"Sensitivity":"invalid"}"#, r#"{"ExcludedApps":["*.exe"]}"#] {
            assert!(Settings::parse(text.as_bytes()).is_err(), "must reject {text}");
        }
        assert!(Settings::parse(&vec![b' '; 65537]).is_err());
    }

    #[test]
    fn missing_and_partial_settings_keep_compatible_defaults_without_writing() {
        assert_eq!(Settings::parse(b"{}").unwrap(), Settings::default());
        let path = std::env::temp_dir().join(format!("hanautomata-settings-missing-{}.json", std::process::id()));
        assert!(!path.exists());
        assert_eq!(Settings::load(&path).unwrap(), Settings::default());
        assert!(!path.exists());
    }

    #[test]
    fn file_load_handles_utf8_bom_and_preserves_invalid_bytes() {
        let path = std::env::temp_dir().join(format!("hanautomata-settings-{}-{}.json", std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        std::fs::write(&path, b"\xef\xbb\xbf{\"InputMode\":\"English\"}").unwrap();
        assert_eq!(Settings::load(&path).unwrap().input_mode, "English");
        std::fs::write(&path, b"invalid\xff").unwrap();
        assert!(Settings::load(&path).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"invalid\xff");
        std::fs::write(&path, vec![b' '; Settings::MAX_BYTES + 1]).unwrap();
        assert!(Settings::load(&path).is_err());
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn configured_pause_is_restored_after_a_private_field() {
        for paused in [true, false] {
            let settings = Settings { learning_paused: paused, ..Settings::default() };
            let path = std::env::temp_dir().join(format!("hanautomata-settings-pause-{}-{paused}.json", std::process::id()));
            let mut shell = Shell::new(Engine::new(String::new(), path.to_string_lossy().into_owned()));
            settings.apply_new_session(&mut shell);
            shell.context_policy(false, true);
            assert!(shell.engine().learning_paused());
            shell.context_policy(false, false);
            assert_eq!(shell.engine().learning_paused(), paused);
            drop(shell);
            let mut marker = path.as_os_str().to_owned(); marker.push(".lock");
            if Path::new(&marker).exists() { std::fs::remove_file(marker).unwrap(); }
        }
    }

    #[test]
    fn csharp_settings_fixture_uses_the_same_contract() {
        let expected = Settings::parse(include_bytes!("../../tests/fixtures/tsf-settings.json")).unwrap();
        assert_eq!(expected.excluded_apps[1], "한글 도구.exe");
        assert!(expected.delete_syllable && expected.ms_clusters && expected.strict);
        if let Some(path) = std::env::var_os("HANAUTOMATA_CSHARP_SETTINGS_TEST") {
            assert_eq!(Settings::load(Path::new(&path)).unwrap(), expected, "actual C# serialization must load unchanged");
        }
    }

    #[test]
    #[cfg(windows)]
    fn excluded_windows_executable_names_ignore_unicode_case_but_not_accents() {
        let settings = Settings { excluded_apps: vec!["Äpp.exe".into(), "한글 도구.exe".into()], ..Settings::default() };
        assert!(settings.excludes("äpp.EXE"));
        assert!(settings.excludes("한글 도구.EXE"));
        assert!(!settings.excludes("app.exe"));
        assert!(!settings.excludes("other-äpp.exe"));
    }

    #[test]
    fn missing_settings_after_a_writer_started_never_reenables_defaults() {
        let path = std::env::temp_dir().join(format!("hanautomata-settings-recovery-{}.json", std::process::id()));
        let mut marker = path.as_os_str().to_owned(); marker.push(".lock");
        std::fs::write(&marker, b"").unwrap();
        let result = Settings::load(&path);
        std::fs::remove_file(marker).unwrap();
        assert!(result.is_err(), "a missing file during/after replacement must not reset app exclusions");
    }
}
