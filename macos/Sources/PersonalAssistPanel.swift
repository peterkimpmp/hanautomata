import Cocoa

final class PersonalAssistPanel: NSObject, NSWindowDelegate, NSTextFieldDelegate, NSTableViewDataSource, NSTableViewDelegate {
    static var current: PersonalAssistPanel?
    let window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 760, height: 580),
        styleMask: [.titled, .closable, .resizable, .miniaturizable], backing: .buffered, defer: false)
    private let service: PersonalAssistClient
    private let copyText: (String) -> Bool
    private let search = NSTextField(string: "")
    private let expression = NSTextField(string: "")
    private let enabled = NSButton(checkboxWithTitle: "추천 사용 · 복사 선택을 순위에 반영", target: nil, action: nil)
    private let status = NSTextField(wrappingLabelWithString: "불러오는 중…")
    private let table = NSTableView()
    private let copyButton = NSButton(title: "복사", target: nil, action: nil)
    private let deleteButton = NSButton(title: "삭제", target: nil, action: nil)
    private let undoButton = NSButton(title: "최근 순위 기억 되돌리기", target: nil, action: nil)
    private let clearButton = NSButton(title: "전체 삭제", target: nil, action: nil)
    private let retryButton = NSButton(title: "다시 불러오기", target: nil, action: nil)
    private let addButton = NSButton(title: "등록", target: nil, action: nil)
    private var items: [AssistItem] = []
    private var busy = false, refreshPending = false, savedEnabled = false
    private var revision = 0, undoID = "", notice = ""
    private var debounce: DispatchWorkItem?
    private let standalone: Bool

    static func show(standalone: Bool = false) {
        if current == nil { current = PersonalAssistPanel(standalone: standalone) }
        guard let current else { return }
        current.window.makeKeyAndOrderFront(nil); NSApp.activate(ignoringOtherApps: true)
    }

    init(standalone: Bool = false, service: PersonalAssistClient? = nil, copyText: ((String) -> Bool)? = nil) {
        self.standalone = standalone
        let folder = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask).first!
        self.service = service ?? PersonalAssistClient(executable: Bundle.main.bundleURL.appendingPathComponent("Contents/MacOS/hanautomata-assist"),
            store: folder.appendingPathComponent("Hanautomata/personal-expressions.json"))
        self.copyText = copyText ?? { text in
            NSPasteboard.general.clearContents()
            return NSPasteboard.general.setString(text, forType: .string)
        }
        super.init()
        window.title = "내 표현 · 이 기기에서 처리"; window.delegate = self
        window.isReleasedWhenClosed = false; window.minSize = NSSize(width: 700, height: 550)
        let heading = NSTextField(labelWithString: "내 단어와 표현을 빠르게 찾으세요")
        heading.font = .systemFont(ofSize: 23, weight: .semibold)
        let description = NSTextField(wrappingLabelWithString: "직접 등록한 표현만 이 기기에 평문으로 저장합니다. 입력 내용은 자동 수집하지 않습니다.")
        description.textColor = .secondaryLabelColor
        search.placeholderString = "표현 찾기 · 두 글자 이상 / 비우면 전체 목록"
        search.delegate = self
        search.setAccessibilityLabel("표현 찾기"); search.setAccessibilityIdentifier("assist-search")
        expression.placeholderString = "새 표현 · 한 줄 2~256자, 최대 512개"
        expression.setAccessibilityLabel("새 표현"); expression.setAccessibilityIdentifier("assist-expression")
        status.setAccessibilityLabel("추천 상태"); status.setAccessibilityIdentifier("assist-status")
        enabled.target = self; enabled.action = #selector(toggle)
        let column = NSTableColumn(identifier: NSUserInterfaceItemIdentifier("expression"))
        column.title = "내 표현"; column.width = 680; table.addTableColumn(column)
        table.delegate = self; table.dataSource = self; table.headerView = nil; table.rowHeight = 30
        table.usesAlternatingRowBackgroundColors = true
        table.setAccessibilityLabel("내 표현 목록"); table.setAccessibilityIdentifier("assist-items")
        let scroll = NSScrollView(); scroll.documentView = table; scroll.hasVerticalScroller = true; scroll.borderType = .bezelBorder
        let actions = NSStackView(views: [copyButton, deleteButton, undoButton, clearButton, retryButton]); actions.spacing = 8
        for (button, action) in [(copyButton, #selector(copyItem)), (deleteButton, #selector(deleteItem)),
            (undoButton, #selector(undo)), (clearButton, #selector(clear)), (retryButton, #selector(reload)), (addButton, #selector(add))] {
            button.target = self; button.action = action; button.setAccessibilityLabel(button.title)
        }
        let register = NSStackView(views: [expression, addButton]); register.spacing = 8
        let hint = NSTextField(wrappingLabelWithString: "추천을 복사한 뒤 원래 앱에서 원하는 곳에 붙여넣으세요. 순위 되돌림은 문서나 클립보드를 바꾸지 않습니다.")
        hint.textColor = .secondaryLabelColor
        let stack = NSStackView(views: [heading, description, enabled, search, scroll, actions, status, register, hint])
        stack.orientation = .vertical; stack.alignment = .leading; stack.spacing = 14
        stack.translatesAutoresizingMaskIntoConstraints = false; window.contentView!.addSubview(stack)
        NSLayoutConstraint.activate([
            stack.leadingAnchor.constraint(equalTo: window.contentView!.leadingAnchor, constant: 22),
            stack.trailingAnchor.constraint(equalTo: window.contentView!.trailingAnchor, constant: -22),
            stack.topAnchor.constraint(equalTo: window.contentView!.topAnchor, constant: 22),
            stack.bottomAnchor.constraint(equalTo: window.contentView!.bottomAnchor, constant: -22),
            scroll.heightAnchor.constraint(greaterThanOrEqualToConstant: 150),
        ] + [description, search, scroll, status, register, hint].map { $0.widthAnchor.constraint(equalTo: stack.widthAnchor) })
        scroll.setContentHuggingPriority(.defaultLow, for: .vertical)
        window.center(); controls(); reload()
    }

    func numberOfRows(in tableView: NSTableView) -> Int { items.count }
    func tableView(_ tableView: NSTableView, viewFor tableColumn: NSTableColumn?, row: Int) -> NSView? {
        guard items.indices.contains(row) else { return nil }
        let label = NSTextField(labelWithString: items[row].text)
        label.lineBreakMode = .byTruncatingTail; label.toolTip = items[row].text
        return label
    }
    func tableViewSelectionDidChange(_ notification: Notification) { controls() }
    func controlTextDidChange(_ obj: Notification) {
        revision += 1; items = []; table.reloadData(); controls()
        debounce?.cancel()
        let work = DispatchWorkItem { [weak self] in self?.reload() }; debounce = work
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.15, execute: work)
    }
    private func controls() {
        let selected = items.indices.contains(table.selectedRow)
        copyButton.isEnabled = !busy && selected; deleteButton.isEnabled = !busy && selected
        undoButton.isEnabled = !busy && !undoID.isEmpty
        for button in [enabled, clearButton, retryButton, addButton] { button.isEnabled = !busy }
        expression.isEnabled = !busy
    }
    @objc private func reload() {
        if busy { refreshPending = true; return }
        let all = search.stringValue.trimmingCharacters(in: .whitespaces).isEmpty
        send(all ? "list" : "query", fields: all ? [:] : ["text": search.stringValue], read: true)
    }
    private func send(_ command: String, fields: [String: Any] = [:], read: Bool = false, message: String = "") {
        guard !busy else { return }
        let stamp = revision; busy = true; controls(); status.stringValue = "불러오는 중…"
        service.send(command, fields: fields) { [weak self] result in
            guard let self else { return }
            self.busy = false
            switch result {
            case .failure:
                self.items = []; self.undoID = ""; self.status.stringValue = PersonalAssistClient.message("connection_failed")
                self.enabled.state = self.savedEnabled ? .on : .off
            case .success(let reply):
                if !reply.ok {
                    self.items = []; self.undoID = ""
                    self.status.stringValue = (command == "copied" ? "복사는 완료됐지만 순위를 저장하지 못했습니다. " : "") + PersonalAssistClient.message(reply.error)
                    self.enabled.state = self.savedEnabled ? .on : .off
                } else {
                    self.savedEnabled = reply.enabled
                    self.enabled.state = reply.enabled ? .on : .off
                    if !read { self.undoID = reply.undo_id; self.notice = message; if command == "add" { self.expression.stringValue = "" } }
                    let all = self.search.stringValue.trimmingCharacters(in: .whitespaces).isEmpty
                    if stamp == self.revision && (read || all) {
                        self.items = reply.items
                        self.status.stringValue = read ? (all ? "저장한 표현 \(reply.items.count)개 · 선택 후 복사하거나 삭제하세요." :
                            !reply.enabled ? "추천 사용을 켜면 등록한 표현을 찾을 수 있습니다." : reply.items.isEmpty ? "일치하는 표현이 없습니다. 두 글자 이상으로 찾아보세요." : "내 표현 추천 \(reply.items.count)개 · 복사해서 사용하세요.") : message
                        if read && !self.notice.isEmpty { self.status.stringValue = self.notice + " " + self.status.stringValue; self.notice = "" }
                    } else { self.refreshPending = true }
                }
            }
            self.table.reloadData()
            if !self.items.isEmpty { self.table.selectRowIndexes(IndexSet(integer: 0), byExtendingSelection: false) }
            self.controls()
            if self.refreshPending || stamp != self.revision { self.refreshPending = false; self.reload() }
        }
    }
    @objc private func toggle() { send("set_enabled", fields: ["enabled": enabled.state == .on], message: "추천 설정을 저장했습니다.") }
    @objc private func add() { send("add", fields: ["text": expression.stringValue], message: "표현을 저장했습니다.") }
    @objc private func deleteItem() {
        guard items.indices.contains(table.selectedRow) else { return }
        send("delete", fields: ["entry_id": items[table.selectedRow].id], message: "표현을 삭제했습니다.")
    }
    @objc private func copyItem() {
        guard !busy, items.indices.contains(table.selectedRow) else { return }
        let item = items[table.selectedRow]
        guard copyText(item.text) else {
            status.stringValue = "클립보드에 복사하지 못했습니다. 순위는 바꾸지 않았습니다."; return
        }
        if enabled.state == .on { send("copied", fields: ["entry_id": item.id], message: "복사했습니다. 선택을 순위에 반영했습니다.") }
        else { status.stringValue = "복사했습니다. 추천이 꺼져 있어 순위는 바꾸지 않았습니다." }
    }
    @objc private func undo() { send("undo", fields: ["undo_id": undoID], message: "최근 순위 기억을 복원했습니다. 문서와 클립보드는 바꾸지 않았습니다.") }
    @objc private func clear() {
        let alert = NSAlert(); alert.messageText = "모든 표현과 순위 기억을 삭제할까요?"
        alert.addButton(withTitle: "취소"); alert.addButton(withTitle: "전체 삭제")
        alert.beginSheetModal(for: window) { [weak self] response in
            if response == .alertSecondButtonReturn { self?.send("clear", message: "모든 표현을 삭제했습니다.") }
        }
    }
    func windowWillClose(_ notification: Notification) {
        debounce?.cancel(); service.close(); Self.current = nil
        if standalone { NSApp.terminate(nil) }
    }
}
