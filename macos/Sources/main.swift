import Carbon
import Cocoa
import InputMethodKit

// `Hanautomata --register` (run by install.sh) registers the bundle with the input-source database and enables it, so it
// appears in the input menu without opening System Settings. `--unregister` disables it (run by uninstall.sh).
let sourceID = "io.github.peterkimpmp.inputmethod.Hanautomata"

func inputSources() -> [TISInputSource] {
    let filter = [kTISPropertyInputSourceID as String: sourceID] as CFDictionary
    return (TISCreateInputSourceList(filter, true)?.takeRetainedValue() as? [TISInputSource]) ?? []
}

if CommandLine.arguments.contains("--register") {
    let status = TISRegisterInputSource(Bundle.main.bundleURL as CFURL)
    if status != noErr { FileHandle.standardError.write(Data("register: status \(status) (already registered is fine)\n".utf8)) }
    let sources = inputSources()
    if sources.isEmpty {
        FileHandle.standardError.write(Data("input source \(sourceID) not found; log out and back in, then run again\n".utf8))
        exit(1)
    }
    for source in sources {
        let status = TISEnableInputSource(source)
        if status != noErr { FileHandle.standardError.write(Data("enable: status \(status)\n".utf8)) }
    }
    // A first install may stay disabled until the user adds it (System Settings) or logs in again; report what is true.
    let enabled = inputSources().contains { source in
        guard let value = TISGetInputSourceProperty(source, kTISPropertyInputSourceIsEnabled) else { return false }
        return CFBooleanGetValue(Unmanaged<CFBoolean>.fromOpaque(value).takeUnretainedValue())
    }
    print(enabled ? "enabled \(sourceID)" : "registered \(sourceID) but not enabled yet")
    exit(enabled ? 0 : 2)
}

if CommandLine.arguments.contains("--unregister") {
    for source in inputSources() { TISDisableInputSource(source) }
    print("disabled \(sourceID)")
    exit(0)
}

guard let connectionName = Bundle.main.infoDictionary?["InputMethodConnectionName"] as? String,
      let bundleIdentifier = Bundle.main.bundleIdentifier
else { fatalError("Info.plist must define InputMethodConnectionName and CFBundleIdentifier") }

let server = IMKServer(name: connectionName, bundleIdentifier: bundleIdentifier)
_ = Shell.engine
NSApplication.shared.run()
