import Cocoa
import IOKit
import IOKit.hid

/// The Caps Lock state and light (IOHIDSystem). A switch tap turns the lock the system set at the press off again.
enum CapsLock {
    private static func withConnection(_ body: (io_connect_t) -> Void) {
        let service = IOServiceGetMatchingService(kIOMainPortDefault, IOServiceMatching(kIOHIDSystemClass))
        guard service != 0 else { return }
        defer { IOObjectRelease(service) }
        var connection: io_connect_t = 0
        guard IOServiceOpen(service, mach_task_self_, UInt32(kIOHIDParamConnectType), &connection) == KERN_SUCCESS else { return }
        defer { IOServiceClose(connection) }
        body(connection)
    }

    static func set(_ on: Bool) {
        withConnection { _ = IOHIDSetModifierLockState($0, Int32(kIOHIDCapsLockState), on) }
    }

    static func isOn() -> Bool {
        var state = false
        withConnection { _ = IOHIDGetModifierLockState($0, Int32(kIOHIDCapsLockState), &state) }
        return state
    }
}

/// Caps Lock press and release from the keyboards themselves, which tell a short press from a long one (M-04). Needs
/// Input Monitoring; without it `start()` fails and the shell uses the lock changes InputMethodKit reports.
final class CapsLockMonitor {
    static let shared = CapsLockMonitor()

    enum Access {
        case granted, denied, unknown
    }

    static var access: Access {
        switch IOHIDCheckAccess(kIOHIDRequestTypeListenEvent) {
        case kIOHIDAccessTypeGranted: return .granted
        case kIOHIDAccessTypeDenied: return .denied
        default: return .unknown
        }
    }

    /// Shows the system prompt once; afterwards the user changes it in System Settings > Privacy & Security.
    @discardableResult
    static func requestAccess() -> Bool {
        IOHIDRequestAccess(kIOHIDRequestTypeListenEvent)
    }

    var onPress: ((TimeInterval, Bool) -> Void)?
    var onRelease: ((TimeInterval) -> Void)?
    private var manager: IOHIDManager?

    var running: Bool { manager != nil }

    /// Starts listening if Input Monitoring is granted; true while running.
    func start() -> Bool {
        if manager != nil { return true }
        guard CapsLockMonitor.access == .granted else { return false }
        let manager = IOHIDManagerCreate(kCFAllocatorDefault, IOOptionBits(kIOHIDOptionsTypeNone))
        IOHIDManagerSetDeviceMatching(manager, [kIOHIDDeviceUsagePageKey: kHIDPage_GenericDesktop,
                                                kIOHIDDeviceUsageKey: kHIDUsage_GD_Keyboard] as CFDictionary)
        IOHIDManagerSetInputValueMatching(manager, [kIOHIDElementUsagePageKey: kHIDPage_KeyboardOrKeypad,
                                                    kIOHIDElementUsageKey: kHIDUsage_KeyboardCapsLock] as CFDictionary)
        let context = Unmanaged.passUnretained(self).toOpaque()
        IOHIDManagerRegisterInputValueCallback(manager, { context, _, _, value in
            guard let context else { return }
            let monitor = Unmanaged<CapsLockMonitor>.fromOpaque(context).takeUnretainedValue()
            let time = ProcessInfo.processInfo.systemUptime
            if IOHIDValueGetIntegerValue(value) != 0 {
                monitor.onPress?(time, NSEvent.modifierFlags.contains(.shift))
            } else {
                monitor.onRelease?(time)
            }
        }, context)
        IOHIDManagerScheduleWithRunLoop(manager, CFRunLoopGetMain(), CFRunLoopMode.commonModes.rawValue)
        guard IOHIDManagerOpen(manager, IOOptionBits(kIOHIDOptionsTypeNone)) == kIOReturnSuccess else {
            IOHIDManagerUnscheduleFromRunLoop(manager, CFRunLoopGetMain(), CFRunLoopMode.commonModes.rawValue)
            return false
        }
        self.manager = manager
        return true
    }

    func stop() {
        guard let manager else { return }
        IOHIDManagerClose(manager, IOOptionBits(kIOHIDOptionsTypeNone))
        IOHIDManagerUnscheduleFromRunLoop(manager, CFRunLoopGetMain(), CFRunLoopMode.commonModes.rawValue)
        self.manager = nil
    }
}
