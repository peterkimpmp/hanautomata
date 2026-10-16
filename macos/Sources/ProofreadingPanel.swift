import Cocoa

struct ProofreadingTarget {
    let read: () -> ProofreadingSelection?
    let replace: (ProofreadingSelection, String) -> Bool
}
private final class ReviewPanel: NSPanel {
    override var canBecomeKey: Bool { false }
    override var canBecomeMain: Bool { false }
}

/// Nonactivating: clicking a suggestion keeps keyboard focus in the document.
final class ProofreadingPanel: NSObject, NSWindowDelegate {
    static var current: ProofreadingPanel?
    private let panel = ReviewPanel(contentRect: NSRect(x: 0, y: 0, width: 610, height: 370),
        styleMask: [.titled, .closable, .nonactivatingPanel], backing: .buffered, defer: false)
    private let status = NSTextField(wrappingLabelWithString: "선택한 글을 확인하고 있습니다.")
    private let original = NSTextField(wrappingLabelWithString: "")
    private let choices = NSPopUpButton(frame: .zero, pullsDown: false)
    private let apply = NSButton(title: "선택한 제안 적용", target: nil, action: nil)
    private let protect = NSButton(title: "이 단어 보호", target: nil, action: nil)
    private let worker = ProofreadingWorker()
    private let target: ProofreadingTarget
    private var selection: ProofreadingSelection?
    private var request: ProofreadingRequest?
    private var report: ProofreadingReport?
    private var rows: [(Int, Int)] = []
    private var protected: [String] = []
    private var running = false
    private var workerStarted = false

    static func show(target: ProofreadingTarget) {
        current?.panel.close()
        let controller = ProofreadingPanel(target: target)
        current = controller
        controller.panel.center(); controller.panel.orderFrontRegardless(); controller.inspect()
    }
    private init(target: ProofreadingTarget) {
        self.target = target
        super.init()
        panel.title = "한국어 표기 제안 · 실험 기능"
        panel.delegate = self; panel.isReleasedWhenClosed = false
        panel.level = .floating; panel.hidesOnDeactivate = false; panel.becomesKeyOnlyIfNeeded = true
        let description = NSTextField(wrappingLabelWithString: "선택한 글을 이 기기에서 검사합니다. 사전에 없는 단어는 확인 후보이며, 오류로 확정하지 않습니다.")
        description.textColor = .secondaryLabelColor
        original.maximumNumberOfLines = 4; original.font = .systemFont(ofSize: 16)
        status.setAccessibilityIdentifier("proofreading-status"); choices.setAccessibilityIdentifier("proofreading-choices")
        let check = NSButton(title: "다시 검사", target: self, action: #selector(inspect))
        let cancel = NSButton(title: "검사 취소", target: self, action: #selector(cancelCheck))
        apply.target = self; apply.action = #selector(applyChoice)
        protect.target = self; protect.action = #selector(protectWord)
        let buttons = NSStackView(views: [check, cancel, protect, apply]); buttons.spacing = 8
        let stack = NSStackView(views: [description, original, status, choices, buttons])
        stack.orientation = .vertical; stack.alignment = .leading; stack.spacing = 18
        stack.translatesAutoresizingMaskIntoConstraints = false; panel.contentView!.addSubview(stack)
        NSLayoutConstraint.activate([
            stack.leadingAnchor.constraint(equalTo: panel.contentView!.leadingAnchor, constant: 22),
            stack.trailingAnchor.constraint(equalTo: panel.contentView!.trailingAnchor, constant: -22),
            stack.topAnchor.constraint(equalTo: panel.contentView!.topAnchor, constant: 22),
            choices.widthAnchor.constraint(equalTo: stack.widthAnchor), original.widthAnchor.constraint(equalTo: stack.widthAnchor),
            description.widthAnchor.constraint(equalTo: stack.widthAnchor), status.widthAnchor.constraint(equalTo: stack.widthAnchor)])
        worker.receive = { [weak self] data in self?.received(data) }
        worker.failed = { [weak self] in self?.workerStarted = false; self?.invalidate("검사기가 종료되었습니다. 다시 검사해 주세요.") }
    }
    private func invalidate(_ message: String) {
        running = false; report = nil; rows = []; choices.removeAllItems()
        apply.isEnabled = false; protect.isEnabled = false; status.stringValue = message
    }
    @objc private func inspect() {
        if let request, running { worker.cancel(request.request_id) }
        invalidate("검사 중… 입력은 계속하실 수 있습니다.")
        guard let snapshot = target.read(), !snapshot.text.isEmpty, snapshot.text.utf8.count <= 16_384 else {
            invalidate("조합을 마친 뒤 검사할 글을 선택해 주세요. 입력 칸이 바뀌면 다시 선택해 주세요."); return
        }
        selection = snapshot; original.stringValue = String(snapshot.text.prefix(240))
        let request = ProofreadingRequest(request_id: UUID().uuidString, document_id: snapshot.document,
            revision: snapshot.revision, text: snapshot.text, protected_words: protected)
        self.request = request; running = true
        do {
            if !workerStarted { try worker.start(); workerStarted = true }
            worker.check(request)
        } catch { workerStarted = false; invalidate("검사기 파일이 없습니다. 맞춤법 패키지가 포함된 빌드를 사용해 주세요.") }
    }
    private func received(_ data: Data) {
        guard let value = try? JSONSerialization.jsonObject(with: data) as? [String: Any], let event = value["event"] as? String else { return }
        if event == "ready" { return }
        guard let request, value["request_id"] as? String == request.request_id, running else { return }
        guard event == "result", let raw = value["report"], let bytes = try? JSONSerialization.data(withJSONObject: raw),
              let report = try? JSONDecoder().decode(ProofreadingReport.self, from: bytes), ProofreadingContract.matches(request, report) else {
            invalidate(event == "cancelled" ? "검사를 취소했습니다." : "검사를 마치지 못했습니다. 다시 검사해 주세요."); return
        }
        running = false; self.report = report
        for (d, item) in report.diagnostics.enumerated() {
            for (c, suggestion) in item.suggestions.enumerated() {
                rows.append((d, c))
                choices.addItem(withTitle: "\(item.original) → \(suggestion) · \(item.rule_id == nil ? "사전 후보" : "규칙 제안")")
            }
        }
        apply.isEnabled = !rows.isEmpty; protect.isEnabled = !rows.isEmpty
        status.stringValue = report.diagnostics.isEmpty
            ? "검사한 \(report.checked_words)개 어절에서 제안을 찾지 못했습니다. 문장 전체의 정확성을 보증하지 않습니다."
            : "확인 후보 \(report.diagnostics.count)개 · 적용할 제안을 직접 선택해 주세요."
        if report.limited { status.stringValue += " 검사 한도에 도달했습니다. 짧게 선택해 주세요." }
    }
    @objc private func cancelCheck() {
        if let request { worker.cancel(request.request_id) }
        invalidate("검사를 취소했습니다.")
    }
    @objc private func protectWord() {
        guard let report, rows.indices.contains(choices.indexOfSelectedItem), protected.count < 256 else { return }
        let word = report.diagnostics[rows[choices.indexOfSelectedItem].0].original
        if !protected.contains(word) { protected.append(word) }; inspect()
    }
    @objc private func applyChoice() {
        guard let request, let report, let selection, rows.indices.contains(choices.indexOfSelectedItem), target.read() == selection else {
            invalidate("원문·선택 영역·입력 칸이 바뀌었습니다. 다시 검사해 주세요."); return
        }
        let row = rows[choices.indexOfSelectedItem]
        guard let replacement = ProofreadingContract.preview(request, report, diagnostic: row.0, candidate: row.1),
              target.replace(selection, replacement) else { invalidate("원문을 바꾸지 못했습니다. 다시 선택해 주세요."); return }
        invalidate("제안 한 개를 적용했습니다. 되돌리기는 원래 앱의 실행 취소를 사용하세요.")
    }
    func windowWillClose(_ notification: Notification) { worker.stop(); ProofreadingPanel.current = nil }
}

/// Developer host: real AppKit selection/replacement/Undo, without installing the system IME.
final class ProofreadingDemo: NSObject, NSTextViewDelegate {
    private var revision = 0
    private let text = NSTextView(frame: NSRect(x: 20, y: 70, width: 670, height: 210))
    private let window = NSWindow(contentRect: NSRect(x: 100, y: 100, width: 710, height: 310),
        styleMask: [.titled, .closable, .resizable], backing: .buffered, defer: false)
    func run() {
        NSApplication.shared.setActivationPolicy(.regular)
        // Standard responder actions are needed for real paste/select-all/Undo in the developer host.
        let menu = NSMenu()
        let appItem = NSMenuItem(); menu.addItem(appItem)
        let appMenu = NSMenu(); appItem.submenu = appMenu
        appMenu.addItem(withTitle: "종료", action: #selector(NSApplication.terminate(_:)), keyEquivalent: "q")
        let editItem = NSMenuItem(); menu.addItem(editItem)
        let editMenu = NSMenu(title: "편집"); editItem.submenu = editMenu
        editMenu.addItem(withTitle: "실행 취소", action: Selector(("undo:")), keyEquivalent: "z")
        editMenu.addItem(withTitle: "복사", action: #selector(NSText.copy(_:)), keyEquivalent: "c")
        editMenu.addItem(withTitle: "붙여넣기", action: #selector(NSText.paste(_:)), keyEquivalent: "v")
        editMenu.addItem(withTitle: "모두 선택", action: #selector(NSText.selectAll(_:)), keyEquivalent: "a")
        NSApplication.shared.mainMenu = menu
        window.title = "Hanautomata · 맞춤법 검증용 편집기"
        text.string = "안녕하세요. 오늘은 일이 잘 됬다. 내일은 할수있다. 이제 되요."
        text.font = .systemFont(ofSize: 20); text.isRichText = false; text.allowsUndo = true; text.delegate = self
        window.contentView!.addSubview(text)
        let button = NSButton(title: "선택한 글 검사", target: self, action: #selector(inspect))
        button.frame = NSRect(x: 20, y: 20, width: 200, height: 34); window.contentView!.addSubview(button)
        window.makeKeyAndOrderFront(nil); window.makeFirstResponder(text)
        text.setSelectedRange(NSRange(location: 0, length: text.string.utf16.count))
        NSApplication.shared.activate(ignoringOtherApps: true); NSApplication.shared.run()
    }
    func textDidChange(_ notification: Notification) { revision += 1 }
    private func selection() -> ProofreadingSelection? {
        let range = text.selectedRange()
        guard window.isKeyWindow, !text.hasMarkedText(), range.length > 0, let selected = Range(range, in: text.string) else { return nil }
        return ProofreadingSelection(text: String(text.string[selected]), document: "demo", revision: String(revision), range: range)
    }
    @objc private func inspect() {
        window.makeFirstResponder(text)
        ProofreadingPanel.show(target: ProofreadingTarget(read: { [weak self] in self?.selection() }, replace: { [weak self] old, new in
            guard let self, self.selection() == old, self.text.shouldChangeText(in: old.range, replacementString: new) else { return false }
            self.text.replaceCharacters(in: old.range, with: new); self.text.didChangeText(); return true
        }))
    }
}
