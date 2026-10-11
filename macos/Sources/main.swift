import Carbon
import Cocoa
import InputMethodKit

// `Hanautomata --register` (run by install.sh) registers the bundle with the input-source database and enables the
// automatic mode, so it appears in the input menu without opening System Settings. `--unregister` disables every mode
// (run by uninstall.sh).

if CommandLine.arguments.contains("--register") {
    let status = TISRegisterInputSource(Bundle.main.bundleURL as CFURL)
    if status != noErr { FileHandle.standardError.write(Data("register: status \(status) (already registered is fine)\n".utf8)) }
    let sources = InputSources.sources(id: ModeID.base)
    if sources.isEmpty {
        FileHandle.standardError.write(Data("input source \(ModeID.base) not found; log out and back in, then run again\n".utf8))
        exit(1)
    }
    for source in sources {
        let status = TISEnableInputSource(source)
        if status != noErr { FileHandle.standardError.write(Data("enable: status \(status)\n".utf8)) }
    }
    // A first install may stay disabled until the user adds it (System Settings) or logs in again; report what is true.
    let enabled = InputSources.isEnabled(ModeID.base)
    print(enabled ? "enabled \(ModeID.base)" : "registered \(ModeID.base) but not enabled yet")
    let fixed = [InputMode.korean, .english].map { ModeID.id($0) }.filter { !InputSources.isEnabled($0) }
    if !fixed.isEmpty { print("optional (menu-bar icons for the fixed modes), not added: \(fixed.joined(separator: ", "))") }
    exit(enabled ? 0 : 2)
}

// `--check` (run by install.sh) reports macOS settings that take a switch key first (C-OPS-07), read only.
if CommandLine.arguments.contains("--check") {
    let check = SystemCheck.current()
    let modes = ModeID.all.map { "\(ModeID.id($0)): \(check.enabledModes.contains($0) ? "added" : "not added")" }
    print("input sources\n  " + modes.joined(separator: "\n  "))
    print("⇧Space: " + (check.shiftSpaceTaken
        ? "taken by the macOS shortcut 'Select the previous/next input source' — change it in System Settings > Keyboard > Keyboard Shortcuts > Input Sources"
        : "free"))
    print("Caps Lock: " + (check.capsLockSwitchesABC
        ? "macOS 'Use the Caps Lock key to switch to and from ABC' is on — turn it off in System Settings > Keyboard > Input Sources > Edit…"
        : "free"))
    print("Caps Lock long press needs Input Monitoring for Hanautomata (asked in its first-run setup)")
    exit(0)
}

// `--diagnose` prints the content-free counters the input method keeps (C-OPS-02); `--diagnose reset` clears them.
if CommandLine.arguments.contains("--diagnose") {
    guard let snapshot = Diagnostics.load(from: Shell.diagnosticsPath) else {
        print("no diagnostics yet at \(Shell.diagnosticsPath.path) (saved once a minute while typing)")
        exit(0)
    }
    if CommandLine.arguments.contains("reset") {
        let fresh = Diagnostics()
        try? fresh.save(to: Shell.diagnosticsPath)
        print("diagnostics cleared (restart the input method or log out for it to start from zero)")
        exit(0)
    }
    print(Diagnostics.report(snapshot))
    exit(0)
}

if CommandLine.arguments.contains("--unregister") {
    for source in InputSources.all() { TISDisableInputSource(source) }
    print("disabled \(ModeID.base) and its modes")
    exit(0)
}

guard let connectionName = Bundle.main.infoDictionary?["InputMethodConnectionName"] as? String,
      let bundleIdentifier = Bundle.main.bundleIdentifier
else { fatalError("Info.plist must define InputMethodConnectionName and CFBundleIdentifier") }

let server = IMKServer(name: connectionName, bundleIdentifier: bundleIdentifier)
_ = Shell.core
Shell.updateCapsMonitor()
Shell.startDiagnostics()
SourceLock.update()
NSApplication.shared.run()
