import Cocoa

/// The window under the caret (C-CMP-02): the candidate, its evidence and keys, a short notice, the new mode (C-MOD-08)
/// or numbered hanja (C-LEX-06). It never takes focus or clicks, and moves above the line when there is no room below,
/// so it does not cover the caret. Text size and appearance follow the settings (C-CMP-10); VoiceOver reads what it shows
/// (C-OPS-06).
final class CandidatePanel: Feedback {
    static let shared = CandidatePanel()

    private let panel: NSPanel
    private let title = NSTextField(labelWithString: "")
    private let detail = NSTextField(labelWithString: "")
    private var fade: DispatchWorkItem?

    private init() {
        panel = NSPanel(contentRect: .zero, styleMask: [.borderless, .nonactivatingPanel], backing: .buffered, defer: true)
        panel.level = .popUpMenu
        panel.isOpaque = false
        panel.backgroundColor = .clear
        panel.hasShadow = true
        panel.ignoresMouseEvents = true
        panel.hidesOnDeactivate = false
        panel.collectionBehavior = [.canJoinAllSpaces, .transient, .ignoresCycle, .fullScreenAuxiliary]
        let background = NSVisualEffectView()
        background.material = .popover
        background.state = .active
        background.wantsLayer = true
        background.layer?.cornerRadius = 6
        background.layer?.masksToBounds = true
        detail.textColor = .secondaryLabelColor
        let stack = NSStackView(views: [title, detail])
        stack.orientation = .vertical
        stack.alignment = .leading
        stack.spacing = 1
        stack.edgeInsets = NSEdgeInsets(top: 4, left: 8, bottom: 5, right: 8)
        stack.translatesAutoresizingMaskIntoConstraints = false
        background.addSubview(stack)
        NSLayoutConstraint.activate([
            stack.leadingAnchor.constraint(equalTo: background.leadingAnchor),
            stack.trailingAnchor.constraint(equalTo: background.trailingAnchor),
            stack.topAnchor.constraint(equalTo: background.topAnchor),
            stack.bottomAnchor.constraint(equalTo: background.bottomAnchor),
        ])
        panel.contentView = background
        panel.setAccessibilityRole(.popover)
        panel.setAccessibilityLabel("한오토마타 후보")
    }

    /// Size steps for the candidate text (C-CMP-10).
    private var scale: CGFloat {
        switch Settings.candidateSize {
        case "small": return 0.85
        case "large": return 1.3
        default: return 1
        }
    }

    func showCandidate(_ text: String, hint: String, near rect: CGRect?) {
        show(text, detail: hint, size: 15, near: rect, seconds: nil)
        announce("\(text), \(hint)")
    }

    func showNotice(_ text: String, near rect: CGRect?, sticky: Bool) {
        show(text, detail: nil, size: 12, near: rect ?? mouse(sticky: sticky), seconds: sticky ? nil : 2)
        announce(text)
    }

    func showMode(_ text: String, near rect: CGRect?) {
        show(text, detail: nil, size: 13, near: rect ?? mouse(sticky: false), seconds: 1)
        announce(text)
    }

    func showChoices(_ items: [String], selected: Int, title text: String, near rect: CGRect?) {
        let line = NSMutableAttributedString()
        let font = NSFont.systemFont(ofSize: 17 * scale)
        for (i, item) in items.enumerated() {
            var attributes: [NSAttributedString.Key: Any] = [.font: font]
            if i == selected {
                attributes[.backgroundColor] = NSColor.selectedContentBackgroundColor
                attributes[.foregroundColor] = NSColor.alternateSelectedControlTextColor
            }
            line.append(NSAttributedString(string: " \(i + 1) \(item) ", attributes: attributes))
        }
        show("", attributed: line, detail: text, size: 17, near: rect, seconds: nil)
        if items.indices.contains(selected) { announce("\(selected + 1)번 \(items[selected])") }
    }

    func hide() {
        fade?.cancel()
        fade = nil
        panel.orderOut(nil)
    }

    private func show(_ text: String, attributed: NSAttributedString? = nil, detail hint: String?, size: CGFloat,
                      near rect: CGRect?, seconds: Double?) {
        fade?.cancel()
        fade = nil
        guard let anchor = rect else { return hide() }
        switch Settings.candidateTheme {
        case "light": panel.appearance = NSAppearance(named: .aqua)
        case "dark": panel.appearance = NSAppearance(named: .darkAqua)
        default: panel.appearance = nil
        }
        title.font = .systemFont(ofSize: size * scale, weight: .medium)
        if let attributed {
            title.attributedStringValue = attributed
        } else {
            title.stringValue = text
        }
        detail.font = .systemFont(ofSize: 11 * scale)
        detail.stringValue = hint ?? ""
        detail.isHidden = hint == nil
        let fitting = panel.contentView?.fittingSize ?? .zero
        let screen = NSScreen.screens.first { $0.frame.intersects(anchor) } ?? NSScreen.main
        let visible = screen?.visibleFrame ?? .infinite
        var origin = CGPoint(x: anchor.minX - 8, y: anchor.minY - fitting.height - 4)
        if origin.y < visible.minY { origin.y = anchor.maxY + 4 }
        origin.x = min(max(origin.x, visible.minX), visible.maxX - fitting.width)
        panel.setFrame(CGRect(origin: origin, size: fitting), display: true)
        panel.orderFrontRegardless()
        if let seconds {
            let work = DispatchWorkItem { [weak self] in self?.hide() }
            fade = work
            DispatchQueue.main.asyncAfter(deadline: .now() + seconds, execute: work)
        }
    }

    /// A notice the client gave no caret position for appears by the mouse; a candidate does not appear at all.
    private func mouse(sticky: Bool) -> CGRect? {
        sticky ? nil : CGRect(origin: NSEvent.mouseLocation, size: CGSize(width: 1, height: 16))
    }

    /// VoiceOver reads candidates and notices as they appear (C-OPS-06).
    private func announce(_ text: String) {
        guard NSWorkspace.shared.isVoiceOverEnabled else { return }
        NSAccessibility.post(element: NSApp as Any, notification: .announcementRequested,
                             userInfo: [.announcement: text, .priority: NSAccessibilityPriorityLevel.high.rawValue])
    }
}
