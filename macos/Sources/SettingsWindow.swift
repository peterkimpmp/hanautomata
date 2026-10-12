import Cocoa
import SwiftUI
import UniformTypeIdentifiers

// Settings window (C-OPS-03 · M-17) and the first-run setup in three steps (C-OPS-07). Every change applies at once in
// this process; macOS settings that take a switch key are detected and explained, never changed.

final class SettingsModel: ObservableObject {
    @Published var shiftSpace = Settings.shiftSpace { didSet { Settings.shiftSpace = shiftSpace } }
    @Published var capsLock = Settings.capsLock {
        didSet {
            Settings.capsLock = capsLock
            refresh()
        }
    }
    @Published var rightCommand = Settings.rightCommand { didSet { Settings.rightCommand = rightCommand } }
    @Published var extraTaps = Settings.extraTaps { didSet { Settings.extraTaps = extraTaps } }
    @Published var switchKind = Settings.switchKind {
        didSet {
            Settings.switchKind = switchKind
            Shell.engine.setSwitchKind(kind: switchKind)
        }
    }
    @Published var pauseKey = Settings.pauseKey { didSet { Settings.pauseKey = pauseKey } }
    @Published var learningPaused = Settings.learningPaused {
        didSet {
            Settings.learningPaused = learningPaused
            Shell.core.updateRecording()
        }
    }
    @Published var mode = Shell.engine.mode() {
        didSet { if mode != Shell.engine.mode() { Shell.core.setMode(mode, client: Shell.activeClient) } }
    }
    @Published var keyboard = Settings.keyboard {
        didSet {
            Settings.keyboard = keyboard
            Shell.core.setKeyboard(keyboard, client: Shell.activeClient)
        }
    }
    @Published var sensitivity = Shell.engine.sensitivity() {
        didSet { Settings.sensitivity = Shell.engine.setSensitivity(name: sensitivity) }
    }
    @Published var semicolon = Settings.semicolon {
        didSet {
            Settings.semicolon = semicolon
            Shell.engine.setSemicolonShortcut(enabled: semicolon)
        }
    }
    @Published var candidateAlways = Settings.candidateAlways { didSet { Settings.candidateAlways = candidateAlways } }
    @Published var deleteSyllable = Settings.deleteSyllable {
        didSet {
            Settings.deleteSyllable = deleteSyllable
            Shell.engine.setDeleteSyllable(enabled: deleteSyllable)
        }
    }
    @Published var msClusters = Settings.msClusters {
        didSet {
            Settings.msClusters = msClusters
            Shell.engine.setMsClusters(enabled: msClusters)
        }
    }
    @Published var caretContext = Settings.caretContext { didSet { Settings.caretContext = caretContext } }
    @Published var searchInline = Settings.searchInline { didSet { Settings.searchInline = searchInline } }
    @Published var optionKey = Settings.optionKey { didSet { Settings.optionKey = optionKey } }
    @Published var wonSign = Settings.wonSign { didSet { Settings.wonSign = wonSign } }
    @Published var learning = Shell.engine.learningEnabled() {
        didSet {
            Settings.learning = learning
            Shell.engine.setLearningEnabled(enabled: learning)
            Shell.saveSoon()
        }
    }
    @Published var excluded = Settings.excludedApps { didSet { Settings.excludedApps = excluded } }
    @Published var perAppMode = Settings.perAppMode { didSet { Settings.perAppMode = perAppMode } }
    @Published var builtInProfiles = Settings.builtInProfiles { didSet { Settings.builtInProfiles = builtInProfiles } }
    @Published var profiles = Settings.profiles { didSet { Settings.profiles = profiles } }
    @Published var escVi = Settings.escVi { didSet { Settings.escVi = escVi } }
    @Published var viApps = Settings.viApps { didSet { Settings.viApps = viApps } }
    @Published var lockSource = Settings.lockSource {
        didSet {
            Settings.lockSource = lockSource
            SourceLock.update()
        }
    }
    @Published var modeNotice = Settings.modeNotice { didSet { Settings.modeNotice = modeNotice } }
    @Published var candidateSize = Settings.candidateSize { didSet { Settings.candidateSize = candidateSize } }
    @Published var candidateTheme = Settings.candidateTheme { didSet { Settings.candidateTheme = candidateTheme } }
    @Published var hanjaExtended = Settings.hanjaExtended { didSet { Settings.hanjaExtended = hanjaExtended } }
    @Published var updateCheck = Settings.updateCheck {
        didSet {
            Settings.updateCheck = updateCheck
            if updateCheck { Updates.checkIfDue() } else { Settings.latestVersion = "" }
        }
    }
    @Published var bindings = Settings.bindings { didSet { Settings.bindings = bindings } }
    @Published var keepText = Shell.engine.keepList().joined(separator: "\n")
    @Published var convertText = Shell.engine.convertList().joined(separator: "\n")
    @Published private(set) var learned: [LearnedItem] = Shell.engine.learnedWords()
    @Published private(set) var system = SystemCheck.current()
    @Published private(set) var capsAccess = CapsLockMonitor.access
    @Published private(set) var capsMonitored = Shell.core.capsMonitored
    @Published var status = ""

    /// Re-reads what macOS decides (shortcuts, permission, added sources), e.g. after a visit to System Settings.
    func refresh() {
        Shell.updateCapsMonitor()
        system = SystemCheck.current()
        capsAccess = CapsLockMonitor.access
        capsMonitored = Shell.core.capsMonitored
        learned = Shell.engine.learnedWords()
    }

    func requestCapsAccess() {
        CapsLockMonitor.requestAccess()
        refresh()
    }

    func binding(_ command: Command) -> KeyBinding? {
        bindings[command] ?? command.defaultBinding
    }

    func tap(_ key: TapKey) -> Binding<Bool> {
        Binding(get: { self.extraTaps.contains(key.rawValue) },
                set: { on in
                    if on { self.extraTaps.insert(key.rawValue) } else { self.extraTaps.remove(key.rawValue) }
                })
    }

    func forget(_ item: LearnedItem) {
        _ = Shell.engine.forgetWord(raw: item.raw, keys: item.keys)
        Shell.saveSoon()
        learned = Shell.engine.learnedWords()
    }

    func saveLists() {
        Lists.save(keep: Shell.engine.parseList(text: keepText), convert: Shell.engine.parseList(text: convertText),
                   Shell.engine)
        keepText = Shell.engine.keepList().joined(separator: "\n")
        convertText = Shell.engine.convertList().joined(separator: "\n")
        status = "예외 목록을 저장했습니다"
    }

    func export() {
        let panel = NSSavePanel()
        panel.nameFieldStringValue = "hanautomata-\(ISO8601DateFormatter().string(from: Date()).prefix(10)).json"
        panel.allowedContentTypes = [.json]
        guard panel.runModal() == .OK, let url = panel.url else { return }
        let text = Shell.engine.exportBundle(settings: Settings.exported())
        do {
            try text.write(to: url, atomically: true, encoding: .utf8)
            status = "내보냈습니다: 기억한 선택 \(Shell.engine.learnedCount())개 · 예외 목록 · 설정"
        } catch {
            status = "내보내지 못했습니다: \(error.localizedDescription)"
        }
    }

    func importBundle() {
        let panel = NSOpenPanel()
        panel.allowedContentTypes = [.json]
        guard panel.runModal() == .OK, let url = panel.url,
              let text = try? String(contentsOf: url, encoding: .utf8)
        else { return }
        let result = Shell.engine.importBundle(text: text)
        guard result.error.isEmpty else {
            status = "가져오지 못했습니다: \(result.error)"
            return
        }
        let applied = Settings.imported(result.settings)
        Lists.save(keep: Shell.engine.keepList(), convert: Shell.engine.convertList(), Shell.engine)
        Shell.reapplySettings()
        Shell.saveSoon()
        status = "가져왔습니다: 단어 \(result.words)개 · 예외 \(result.keep + result.convert)개 · 설정 \(applied)개"
        SettingsWindow.reload()
    }
}

private struct Warning: View {
    let text: String
    var button: String?
    var action: () -> Void = {}

    var body: some View {
        HStack(alignment: .top, spacing: 8) {
            Image(systemName: "exclamationmark.triangle.fill").foregroundStyle(.orange)
            VStack(alignment: .leading, spacing: 4) {
                Text(text).fixedSize(horizontal: false, vertical: true)
                if let button { Button(button, action: action) }
            }
        }
        .font(.callout)
    }
}

private struct Caption: View {
    let text: String
    init(_ text: String) { self.text = text }
    var body: some View {
        Text(text).font(.caption).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
    }
}

struct SwitchKeysSection: View {
    @ObservedObject var model: SettingsModel

    var body: some View {
        Section("한/영 전환 키") {
            Toggle("⇧Space — Shift를 누른 뒤 다른 키 없이 Space", isOn: $model.shiftSpace)
            if model.shiftSpace && model.system.shiftSpaceTaken {
                Warning(text: "macOS 단축키 '입력 메뉴에서 이전/다음 소스 선택'이 ⇧Space라서 이 키가 한오토마타까지 오지 않습니다. "
                            + "시스템 설정 > 키보드 > 키보드 단축키 > 입력 소스에서 그 단축키를 바꾸거나 끄세요.",
                        button: "키보드 설정 열기") { SystemSettings.open(SystemSettings.keyboard) }
            }
            Toggle("Caps Lock — 짧게 누르면 전환, 길게 누르거나 Shift+Caps Lock은 대문자 고정", isOn: $model.capsLock)
            if model.capsLock {
                if model.system.capsLockSwitchesABC {
                    Warning(text: "macOS의 'Caps Lock 키로 ABC 입력 소스 전환'이 켜져 있으면 시스템이 Caps Lock을 먼저 가져갑니다. "
                                + "시스템 설정 > 키보드 > 입력 소스 > 편집…에서 끄세요.",
                            button: "키보드 설정 열기") { SystemSettings.open(SystemSettings.keyboard) }
                }
                capsAccess
            }
            Toggle("오른쪽 ⌘ — 혼자 짧게 눌렀다 떼기", isOn: $model.rightCommand)
            Picker("전환 키가 하는 일", selection: $model.switchKind) {
                Text("단어 하나").tag(SwitchKind.singleShot)
                Text("모드 순환").tag(SwitchKind.cycle)
                Text("한↔영 토글").tag(SwitchKind.toggle)
            }
            .pickerStyle(.segmented)
            Caption(switchHelp)
        }
    }

    private var switchHelp: String {
        switch model.switchKind {
        case .singleShot:
            return "자동 판별 모드에서 전환 키는 지금 쓰는 단어만 반대 언어로 바꿉니다. 한 번 더 누르면 원래대로 돌아가고, "
                + "조합 중인 단어가 없으면 다음 단어를 예약합니다. 한글·영문 고정 모드에서는 두 모드를 오갑니다."
        case .cycle:
            return "자동 판별 → 한글 고정 → 영문 고정 → 자동 판별 순서로 모드를 바꿉니다. 조합 중인 단어는 먼저 확정합니다."
        case .toggle:
            return "한글 고정과 영문 고정을 오갑니다(기존 입력기 방식). 자동 판별 모드에서 누르면 지금 화면의 반대 언어로 고정합니다."
        }
    }

    @ViewBuilder private var capsAccess: some View {
        if model.capsMonitored {
            Label("입력 모니터링 허용됨 — 길게 누르면 대문자 고정", systemImage: "checkmark.circle").font(.callout)
        } else {
            VStack(alignment: .leading, spacing: 4) {
                Text("입력 모니터링 권한이 없어도 짧게 눌러 전환하고 Shift+Caps Lock으로 대문자를 고정합니다. "
                     + "길게 눌러 대문자를 고정하려면 권한이 필요합니다. 한오토마타는 Caps Lock만 읽고 다른 키는 기록하지 않습니다.")
                    .font(.callout).fixedSize(horizontal: false, vertical: true)
                HStack {
                    if model.capsAccess == .unknown {
                        Button("권한 요청") { model.requestCapsAccess() }
                    }
                    Button("입력 모니터링 설정 열기") { SystemSettings.open(SystemSettings.inputMonitoring) }
                }
            }
        }
    }
}

struct MoreSwitchKeysSection: View {
    @ObservedObject var model: SettingsModel

    var body: some View {
        Section("전환 키 더하기") {
            Toggle("왼쪽 Shift 단독 탭", isOn: model.tap(.leftShift))
            Toggle("오른쪽 Shift 단독 탭", isOn: model.tap(.rightShift))
            Toggle("오른쪽 ⌥ 단독 탭", isOn: model.tap(.rightOption))
            Toggle("오른쪽 ⌃ 단독 탭", isOn: model.tap(.rightControl))
            Toggle("🌐(fn) 단독 탭", isOn: model.tap(.function))
            Caption("탭은 0.3초 안에 다른 키 없이 눌렀다 뗄 때만 전환합니다. 빠른 대문자 입력과 겹칠 수 있어 기본은 끔입니다. "
                    + "🌐 키는 시스템 설정 > 키보드의 '🌐 키를 눌러'가 '아무것도 안 함'일 때 쓰세요.")
            KeyRecorder(command: .switchKey, title: "사용자 지정 전환 키", model: model)
        }
    }
}

struct InputSection: View {
    @ObservedObject var model: SettingsModel

    var body: some View {
        Section("입력") {
            Picker("모드", selection: $model.mode) {
                ForEach(ModeID.all, id: \.self) { Text(modeName($0)).tag($0) }
            }
            .pickerStyle(.segmented)
            Picker("자판", selection: $model.keyboard) {
                Text("두벌식").tag(KeyboardKind.dubeolsik)
                Text("세벌식 390").tag(KeyboardKind.sebeolsik390)
                Text("세벌식 최종").tag(KeyboardKind.sebeolsikFinal)
            }
            .pickerStyle(.segmented)
            if model.keyboard != .dubeolsik {
                Caption("세벌식은 자동 판별 없이 한글로 조합합니다(자동 판별은 두벌식 전용). 전환 키를 '한↔영 토글'로 두기를 권합니다.")
                if model.switchKind != .toggle { Button("전환 키를 한↔영 토글로") { model.switchKind = .toggle } }
            }
            Picker("판별 민감도", selection: $model.sensitivity) {
                Text("보수").tag("conservative")
                Text("표준").tag("balanced")
                Text("적극").tag("aggressive")
            }
            .pickerStyle(.segmented)
            Toggle("단어 뒤 ; 로 후보 바꾸기 (;; 는 실제 ;)", isOn: $model.semicolon)
            Toggle("후보창을 늘 표시 (끄면 '확인 필요'일 때만)", isOn: $model.candidateAlways)
            Toggle("커서 앞 문장의 언어로 애매한 첫 단어 보정 (실험)", isOn: $model.caretContext)
            if model.caretContext {
                Caption("커서를 옮긴 뒤 첫 단어가 애매하면 앞 문장 언어를 따릅니다. 검사에서 한글 단어 놓침은 1,000개 중 15→6으로 줄었고, "
                        + "한글 문장 속 영어 단어 1,000개 중 4개가 한글로 바뀌었습니다.")
            }
            Toggle("검색 칸에서 조합 중인 글자도 바로 검색되게 (실험)", isOn: $model.searchInline)
            if model.searchInline {
                Caption("검색 칸에서는 밑줄 조합 대신 글자를 바로 넣고 바뀔 때마다 고칩니다. 검색 칸을 알아보려면 시스템 설정 > 개인정보 보호 및 보안 > "
                        + "손쉬운 사용에서 Hanautomata를 켜야 합니다. 지금: " + (FieldInspector.trusted ? "허용됨" : "허용 안 됨"))
                if !FieldInspector.trusted {
                    Button("손쉬운 사용 설정 열기") {
                        if let url = URL(string: "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility") {
                            NSWorkspace.shared.open(url)
                        }
                    }
                }
            }
            Toggle("Backspace로 글자(음절) 단위 지우기 (끄면 낱자 단위)", isOn: $model.deleteSyllable)
            Toggle("자음만 이어 치면 겹자음으로 (ㄱ+ㅅ → ㄳ, Windows 방식)", isOn: $model.msClusters)
            Picker("⌥ + 키", selection: $model.optionKey) {
                Text("특수 문자").tag(KeyMap.OptionKey.special)
                Text("⌥ 무시").tag(KeyMap.OptionKey.ignore)
                Text("영문 글자").tag(KeyMap.OptionKey.english)
            }
            .pickerStyle(.segmented)
            Toggle("한글 고정 모드에서 ` 키로 ₩ 입력", isOn: $model.wonSign)
            Toggle("F12로 일시정지(그대로 입력) 켜고 끄기", isOn: $model.pauseKey)
        }
    }
}

/// Records a key for a command (C-COR-07 · C-MOD-12): press the button, then the key; Esc cancels.
struct KeyRecorder: View {
    let command: Command
    let title: String
    @ObservedObject var model: SettingsModel
    @State private var monitor: Any?

    var body: some View {
        HStack {
            Text(title)
            Spacer()
            Button(monitor != nil ? "키를 누르세요… (Esc 취소)" : (model.binding(command)?.label ?? "없음")) {
                monitor == nil ? start() : stop()
            }
            Menu {
                if let binding = command.defaultBinding { Button("기본값 (\(binding.label))") { set(binding) } }
                Button("없음") { model.bindings[command] = .some(nil) }
            } label: { Image(systemName: "ellipsis.circle") }
                .menuStyle(.borderlessButton).fixedSize()
        }
    }

    private func set(_ binding: KeyBinding?) {
        model.bindings[command] = binding == command.defaultBinding ? nil : .some(binding)
        if binding == command.defaultBinding { model.bindings.removeValue(forKey: command) }
    }

    private func start() {
        monitor = NSEvent.addLocalMonitorForEvents(matching: .keyDown) { event in
            if event.keyCode != 53 {
                let flags = event.modifierFlags
                set(KeyBinding(event.keyCode, KeyBinding.modifiers(shift: flags.contains(.shift),
                                                                    control: flags.contains(.control),
                                                                    option: flags.contains(.option),
                                                                    command: flags.contains(.command))))
            }
            stop()
            return nil
        }
    }

    private func stop() {
        if let monitor { NSEvent.removeMonitor(monitor) }
        monitor = nil
    }
}

struct KeysSection: View {
    @ObservedObject var model: SettingsModel

    var body: some View {
        Section("교정 키") {
            KeyRecorder(command: .candidate, title: "후보 바꾸기 · 직전 단어 재변환", model: model)
            KeyRecorder(command: .flip, title: "선택한 글자 한↔영", model: model)
            KeyRecorder(command: .recent, title: "최근 세 단어 한↔영", model: model)
            KeyRecorder(command: .hanja, title: "한자로 바꾸기", model: model)
            KeyRecorder(command: .pause, title: "일시정지 켜고 끄기", model: model)
            Caption("F2·F12는 키보드 설정에 따라 fn을 함께 눌러야 할 수 있습니다. 탐색기 이름 바꾸기·코드 편집기 등과 겹치면 다른 키로 "
                    + "바꾸세요. 바꾸면 원래 키는 앱에 그대로 갑니다.")
        }
        Section("모드 바로 가기") {
            KeyRecorder(command: .modeAuto, title: "자동 판별로", model: model)
            KeyRecorder(command: .modeKorean, title: "한글 고정으로", model: model)
            KeyRecorder(command: .modeEnglish, title: "영문 고정으로", model: model)
        }
        Section("한자") {
            Toggle("확장 한자까지 보이기 (KS X 1002·인명용)", isOn: $model.hanjaExtended)
            Caption("한글 한 글자 뒤에서, 또는 한 글자를 선택하고 한자 키를 누르면 후보가 뜹니다. 숫자·↩로 고르고 Esc로 취소합니다. "
                    + "자료: Unicode Unihan.")
        }
    }
}

struct AppsSection: View {
    @ObservedObject var model: SettingsModel

    var body: some View {
        Section("앱마다") {
            Toggle("앱마다 마지막 모드 기억", isOn: $model.perAppMode)
            Toggle("기본 앱 프로필 쓰기 — 원격·가상 머신은 영문 고정으로 시작, 터미널·코드 편집기는 보수 판별",
                   isOn: $model.builtInProfiles)
            Toggle("입력 소스 잠금 — 다른 입력 소스로 바뀌면 한오토마타로 되돌리기", isOn: $model.lockSource)
            Picker("Esc로 영문 전환(vi)", selection: $model.escVi) {
                Text("끔").tag("off")
                Text("터미널·편집기").tag("apps")
                Text("모든 앱").tag("all")
            }
            .pickerStyle(.segmented)
            Caption("vi 모드: Esc·⌃[가 조합을 확정하고 영문 고정으로 바꾼 뒤 앱에 그대로 갑니다. 전환 키를 누르면 이전 모드로 돌아갑니다.")
        }
        Section("앱 프로필") {
            ForEach(model.profiles.keys.sorted(), id: \.self) { id in
                ProfileRow(id: id, model: model)
            }
            Menu("실행 중인 앱 추가") {
                ForEach(runningApps, id: \.0) { app in
                    Button(app.1) { model.profiles[app.0] = Settings.profile(for: app.0) ?? AppProfile() }
                }
            }
            Caption("프로필은 앱이 처음 초점을 받을 때의 모드와, 그 앱에서 쓰는 민감도·보호 강화를 정합니다. "
                    + "보호 강화는 통계 추정만으로는 한글로 바꾸지 않고 확인을 받습니다.")
        }
        Section("이 앱에서는 끄기") {
            if model.excluded.isEmpty {
                Text("없음 — 그 앱에서 입력 메뉴의 '…에서 끄기'로 추가합니다.").foregroundStyle(.secondary)
            }
            ForEach(model.excluded, id: \.self) { id in
                HStack {
                    VStack(alignment: .leading) {
                        Text(InputSources.appName(id))
                        Text(id).font(.caption).foregroundStyle(.secondary)
                    }
                    Spacer()
                    Button("다시 켜기") { model.excluded.removeAll { $0 == id } }
                }
            }
        }
    }

    private var runningApps: [(String, String)] {
        NSWorkspace.shared.runningApplications
            .filter { $0.activationPolicy == .regular }
            .compactMap { app in app.bundleIdentifier.map { ($0, app.localizedName ?? $0) } }
            .filter { model.profiles[$0.0] == nil }
            .sorted { $0.1 < $1.1 }
    }
}

private struct ProfileRow: View {
    let id: String
    @ObservedObject var model: SettingsModel

    var body: some View {
        let profile = Binding(get: { model.profiles[id] ?? AppProfile() }, set: { model.profiles[id] = $0 })
        VStack(alignment: .leading, spacing: 4) {
            HStack {
                Text(InputSources.appName(id)).bold()
                Text(id).font(.caption).foregroundStyle(.secondary)
                Spacer()
                Button("삭제") { model.profiles.removeValue(forKey: id) }
            }
            HStack {
                Picker("시작 모드", selection: profile.mode) {
                    Text("기본").tag(String?.none)
                    Text("자동").tag(String?.some("auto"))
                    Text("한글").tag(String?.some("korean"))
                    Text("영문").tag(String?.some("english"))
                }
                Picker("민감도", selection: profile.sensitivity) {
                    Text("기본").tag(String?.none)
                    Text("보수").tag(String?.some("conservative"))
                    Text("표준").tag(String?.some("balanced"))
                    Text("적극").tag(String?.some("aggressive"))
                }
                Toggle("보호 강화", isOn: profile.strict)
            }
        }
    }
}

struct DictionarySection: View {
    @ObservedObject var model: SettingsModel
    @State private var filter = ""

    var body: some View {
        Section("학습") {
            Toggle("고른 후보를 기억", isOn: $model.learning)
            if model.learning {
                Toggle("학습 일시 중지 — 기억한 선택은 계속 쓰고 새로 기억하지 않음(공용 Mac·시연용)", isOn: $model.learningPaused)
            }
            Caption("F2: 조합 중이면 후보 바꾸기, Space로 확정한 직후면 그 단어를 바꾸고 기억 · Shift+F2: 선택한 글자의 한글↔영타 뒤집기")
        }
        Section("기억한 선택 \(model.learned.count)개") {
            TextField("찾기", text: $filter)
            List {
                ForEach(model.learned.filter { filter.isEmpty || $0.raw.contains(filter) || $0.hangul.contains(filter) },
                        id: \.raw) { item in
                    HStack {
                        Text(item.korean ? item.hangul : item.raw).bold()
                        Text(item.korean ? "← \(item.raw)" : "(한글로 안 바꿈: \(item.hangul))").foregroundStyle(.secondary)
                        if item.contexts > 0 { Text("문맥 \(item.contexts)").font(.caption).foregroundStyle(.secondary) }
                        Spacer()
                        Button("지우기") { model.forget(item) }
                    }
                }
            }
            .frame(minHeight: 120, maxHeight: 200)
        }
        Section("예외 목록") {
            Text("절대 바꾸지 않을 단어(영문 그대로) — 한 줄에 하나").font(.callout)
            TextEditor(text: $model.keepText).font(.body.monospaced()).frame(height: 70)
            Text("항상 한글로 바꿀 단어 — 한글(안녕) 또는 영타(dkssud)로").font(.callout)
            TextEditor(text: $model.convertText).font(.body.monospaced()).frame(height: 70)
            HStack {
                Button("저장") { model.saveLists() }
                Button("파일 보기") { NSWorkspace.shared.activateFileViewerSelecting([Lists.keepURL, Lists.convertURL]) }
            }
            Caption("목록은 \(Lists.folder.path)의 keep.txt·convert.txt에도 있습니다. 파일을 고치면 다음 입력부터 반영됩니다.")
        }
        Section("가져오기·내보내기") {
            HStack {
                Button("내보내기…") { model.export() }
                Button("가져오기…") { model.importBundle() }
            }
            Caption("기억한 선택·예외 목록·설정을 한 파일로 주고받습니다. Windows판과 같은 형식입니다. 가져오면 지금 것에 더합니다.")
        }
    }
}

struct DisplaySection: View {
    @ObservedObject var model: SettingsModel

    var body: some View {
        Section("표시") {
            Toggle("모드가 바뀌면 커서 옆에 잠깐 표시", isOn: $model.modeNotice)
            Picker("후보창 글자 크기", selection: $model.candidateSize) {
                Text("작게").tag("small")
                Text("보통").tag("medium")
                Text("크게").tag("large")
            }
            .pickerStyle(.segmented)
            Picker("후보창 모양", selection: $model.candidateTheme) {
                Text("시스템").tag("system")
                Text("밝게").tag("light")
                Text("어둡게").tag("dark")
            }
            .pickerStyle(.segmented)
        }
        IconsSection(model: model)
        Section("업데이트") {
            Toggle("하루 한 번 새 판 확인 (GitHub Releases)", isOn: $model.updateCheck)
            Caption("끄면 한오토마타는 네트워크를 전혀 쓰지 않습니다. 켜도 입력한 글자는 보내지 않습니다.")
        }
        Section("진단") {
            Text(Diagnostics.report(Shell.core.diagnostics.snapshot))
                .font(.caption.monospaced()).textSelection(.enabled).fixedSize(horizontal: false, vertical: true)
            HStack {
                Button("초기화") {
                    Shell.core.diagnostics.reset()
                    Shell.saveDiagnostics()
                    model.status = "진단 집계를 지웠습니다"
                }
            }
            Caption("입력한 글자 없이 횟수만 셉니다(판정·확정 경로·지연·앱별 통과 사유). 터미널에서 Hanautomata --diagnose로도 봅니다.")
        }
    }
}

struct IconsSection: View {
    @ObservedObject var model: SettingsModel

    var body: some View {
        Section("메뉴 막대 모드 아이콘") {
            ForEach(ModeID.all, id: \.self) { mode in
                HStack {
                    Text(modeName(mode))
                    Spacer()
                    Text(model.system.enabledModes.contains(mode) ? "추가됨" : "추가 안 됨")
                        .foregroundStyle(model.system.enabledModes.contains(mode) ? Color.secondary : Color.orange)
                }
            }
            Caption("고정 모드의 아이콘(가·A)을 메뉴 막대에 보이려면 시스템 설정 > 키보드 > 입력 소스 > 편집… > + > 한국어에서 "
                    + "'한오토마타 (한글 고정)'·'한오토마타 (영문 고정)'도 추가하세요. 추가하지 않아도 모드는 바뀌고 입력 메뉴에 표시됩니다.")
            Button("키보드 설정 열기") { SystemSettings.open(SystemSettings.keyboard) }
        }
    }
}

struct SettingsView: View {
    @ObservedObject var model: SettingsModel

    var body: some View {
        VStack(spacing: 0) {
            TabView {
                Form {
                    SwitchKeysSection(model: model)
                    MoreSwitchKeysSection(model: model)
                }
                .formStyle(.grouped).tabItem { Text("전환 키") }
                Form { InputSection(model: model) }.formStyle(.grouped).tabItem { Text("입력") }
                Form { KeysSection(model: model) }.formStyle(.grouped).tabItem { Text("단축키") }
                Form { AppsSection(model: model) }.formStyle(.grouped).tabItem { Text("앱") }
                Form { DictionarySection(model: model) }.formStyle(.grouped).tabItem { Text("사전") }
                Form { DisplaySection(model: model) }.formStyle(.grouped).tabItem { Text("표시·기타") }
            }
            if !model.status.isEmpty {
                Text(model.status).font(.callout).foregroundStyle(.secondary).padding(.bottom, 8)
            }
        }
        .padding(.top, 8)
        .frame(width: 660, height: 720)
        .onReceive(NotificationCenter.default.publisher(for: NSApplication.didBecomeActiveNotification)) { _ in model.refresh() }
    }
}

/// First run: switch keys → mode and layout → what macOS still needs, then typing works (C-OPS-07).
struct SetupView: View {
    @ObservedObject var model: SettingsModel
    let finish: () -> Void
    @State private var step = 0

    private let titles = ["전환 키를 고르세요", "입력 방식을 고르세요", "확인할 것"]

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            Text("한오토마타 시작하기 \(step + 1)/3 — \(titles[step])").font(.title3.bold())
            Form {
                switch step {
                case 0: SwitchKeysSection(model: model)
                case 1: InputSection(model: model)
                default:
                    Section("준비") {
                        if !model.system.shiftSpaceTaken && !model.system.capsLockSwitchesABC {
                            Label("전환 키와 겹치는 macOS 설정이 없습니다.", systemImage: "checkmark.circle")
                        } else {
                            Warning(text: "1단계의 주황색 안내를 처리해야 그 전환 키가 동작합니다.")
                        }
                        Toggle("하루 한 번 새 판 확인 (끄면 네트워크를 쓰지 않음)", isOn: $model.updateCheck)
                        Text("언제든 입력 메뉴 > 설정…에서 바꿀 수 있습니다.").foregroundStyle(.secondary)
                    }
                    IconsSection(model: model)
                }
            }
            .formStyle(.grouped)
            HStack {
                if step > 0 { Button("이전") { step -= 1 } }
                Spacer()
                Button(step < 2 ? "다음" : "시작하기") {
                    if step < 2 {
                        step += 1
                        model.refresh()
                    } else {
                        finish()
                    }
                }
                .keyboardShortcut(.defaultAction)
            }
        }
        .padding(20)
        .frame(width: 620, height: 640)
        .onReceive(NotificationCenter.default.publisher(for: NSApplication.didBecomeActiveNotification)) { _ in model.refresh() }
    }
}

enum SettingsWindow {
    private static var window: NSWindow?

    static func show(firstRun: Bool) {
        let model = SettingsModel()
        let window = self.window ?? {
            let window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 660, height: 720), styleMask: [.titled, .closable],
                                  backing: .buffered, defer: false)
            window.isReleasedWhenClosed = false
            return window
        }()
        self.window = window
        let root = firstRun
            ? AnyView(SetupView(model: model) { window.close() })
            : AnyView(SettingsView(model: model))
        window.contentViewController = NSHostingController(rootView: root)
        window.title = firstRun ? "한오토마타 시작하기" : "한오토마타 설정"
        window.center()
        NSApp.activate(ignoringOtherApps: true)
        window.makeKeyAndOrderFront(nil)
    }

    /// Rebuilds the open settings window from the current settings (after an import).
    static func reload() {
        guard let window, window.isVisible else { return }
        DispatchQueue.main.async { show(firstRun: false) }
    }
}
