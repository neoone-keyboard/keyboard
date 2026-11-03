import UIKit
import os
import KeyboardCore
import MLEngine

final class KeyboardViewController: UIInputViewController {
    private let log = Diagnostics(category: "keyboard")
    /// Largest footprint seen since the keyboard loaded, and when it was last logged.
    private var peakFootprint: UInt64 = 0
    private var lastFootprintSample: Double = 0
    private static let footprintSampleInterval: Double = 5
    /// How the last delivered fetch ended, for explaining an empty bar.
    private var lastOutcome: (outcome: SuggestionCoordinator.Outcome, elapsed: Duration)?
    /// The empty-bar reason last logged, so a repeated render isn't logged twice.
    private var loggedEmptyBar: String?
    private var gate = PredictionGate(budget: .keyboardExtension)
    private let topBar = TopBarView()
    private let keyboardView = KeyboardView()
    private let settingsPanel = SettingsPanelView()
    private var heightConstraint: NSLayoutConstraint?
    private var settings = KeyboardSettings()
    private var theme = KeyboardTheme.resolve(KeyboardSettings(), style: .light)

    private let provider = EngineSuggestionProvider.shared
    private var lastRequest: SuggestionRequest?
    private var latest = SuggestionResult.empty
    /// A request was sent and neither its result nor its timeout has arrived.
    private var awaitingResult = false
    /// The word the bar's slots were made for (straight apostrophes). A tap on
    /// a slot made for another word is ignored.
    private var barWord: String?

    private lazy var output = ProxyTextOutput { [unowned self] in self.textDocumentProxy }
    private lazy var input = InputController(output: output)

    private lazy var suggestions = SuggestionCoordinator(provider: provider) { [weak self] result in
        guard let self else { return }
        awaitingResult = false
        latest = result
        if !result.isFor(word: Self.straight(input.currentWord)), lastOutcome?.outcome == .result {
            // Lengths only: no typed text in the diagnostics.
            log.info("Stale result: for a \(result.typed.count)-char word but the word is \(input.currentWord.count) chars; \(result.candidates.count) candidates dropped")
        }
        input.updateSuggestions(result)
        refreshTopBar()
    }

    deinit {
        Diagnostics(category: "keyboard").info("Keyboard released; footprint \(Diagnostics.footprint())")
    }

    /// Key clicks need the input view itself to adopt UIInputViewAudioFeedback.
    override func loadView() {
        let root = KeyboardInputView(frame: .zero, inputViewStyle: .keyboard)
        inputView = root
        view = root
    }

    override func viewDidLoad() {
        super.viewDidLoad()
        suggestions.onOutcome = { [weak self] outcome, elapsed in
            guard let self else { return }
            lastOutcome = (outcome, elapsed)
            switch outcome {
            case .timedOut:
                log.warning("Suggest timed out after \(Diagnostics.milliseconds(elapsed)); word \(input.currentWord.count) chars")
            case .failed:
                log.warning("Suggest failed after \(Diagnostics.milliseconds(elapsed)); word \(input.currentWord.count) chars")
            case .result, .cancelled:
                break
            }
        }
        keyboardView.onEvent = { [unowned self] in handle($0) }
        topBar.onVowelSign = { [unowned self] sign in
            let page = input.page, shift = input.shift
            output.beginEdit()
            input.handle(.character(sign), at: ProcessInfo.processInfo.systemUptime)
            afterInput(pageBefore: page, shiftBefore: shift)
        }
        topBar.onSuggestion = { [unowned self] slot in
            // The bar may still show another word's slots while a result is on its way.
            guard barWord == Self.straight(input.currentWord) else { return }
            let page = input.page, shift = input.shift
            output.beginEdit()
            input.commitSuggestion(slot)
            afterInput(pageBefore: page, shiftBefore: shift)
        }
        // Learning runs on the provider's actor; the main thread never waits for it.
        input.onLearningEvent = { [provider] event in
            Task { await provider.handle(event) }
        }

        settingsPanel.onChange = { [unowned self] changed in
            settings = changed
            applySettings()
        }
        settingsPanel.onDone = { [unowned self] in hideSettings() }
        settingsPanel.onInsertDiagnostics = { [unowned self] in insertDiagnostics() }
        // The actor clears the log and the in-memory store; the main thread doesn't wait.
        settingsPanel.onForgetLearned = { [provider] in
            Task { await provider.forgetLearned() }
        }

        let stack = UIStackView(arrangedSubviews: [topBar, keyboardView])
        stack.axis = .vertical
        stack.spacing = 4
        stack.translatesAutoresizingMaskIntoConstraints = false
        view.addSubview(stack)
        settingsPanel.translatesAutoresizingMaskIntoConstraints = false
        settingsPanel.isHidden = true
        view.addSubview(settingsPanel)

        let height = view.heightAnchor.constraint(equalToConstant: 270)
        height.priority = .defaultHigh + 1
        heightConstraint = height
        NSLayoutConstraint.activate([
            height,
            topBar.heightAnchor.constraint(equalToConstant: 40),
            stack.leadingAnchor.constraint(equalTo: view.leadingAnchor, constant: 3),
            stack.trailingAnchor.constraint(equalTo: view.trailingAnchor, constant: -3),
            stack.topAnchor.constraint(equalTo: view.topAnchor, constant: 4),
            stack.bottomAnchor.constraint(equalTo: view.safeAreaLayoutGuide.bottomAnchor, constant: -2),
            settingsPanel.leadingAnchor.constraint(equalTo: keyboardView.leadingAnchor),
            settingsPanel.trailingAnchor.constraint(equalTo: keyboardView.trailingAnchor),
            settingsPanel.topAnchor.constraint(equalTo: keyboardView.topAnchor),
            settingsPanel.bottomAnchor.constraint(equalTo: keyboardView.bottomAnchor),
        ])

        registerForTraitChanges([UITraitUserInterfaceStyle.self]) { (self: Self, _: UITraitCollection) in
            self.applySettings()
        }
        log.info("Keyboard loaded; \(Diagnostics.buildDescription); iOS \(UIDevice.current.systemVersion); footprint \(Diagnostics.footprint())")
    }

    override func viewWillAppear(_ animated: Bool) {
        super.viewWillAppear(animated)
        settings = KeyboardSettings.load(from: .standard)
        hideSettings()
        noteExternalChange()
        applySettings()
    }

    /// The keys are showing: open the lexicons now, off the main thread, so the
    /// first word typed already has suggestions.
    override func viewDidAppear(_ animated: Bool) {
        super.viewDidAppear(animated)
        log.info("Appeared; prediction \(suggestions.isEnabled ? "on" : "off"); footprint \(Diagnostics.footprint())")
        guard suggestions.isEnabled else { return }
        // The provider is shared: an earlier session may have shed it for memory.
        Task { [provider] in
            await provider.resume()
            await provider.warmUp()
        }
    }

    override func viewWillLayoutSubviews() {
        if input.showsNextKeyboard != needsInputModeSwitchKey {
            input.showsNextKeyboard = needsInputModeSwitchKey
            reloadKeys()
        }
        super.viewWillLayoutSubviews()
    }

    override func textDidChange(_ textInput: UITextInput?) {
        super.textDidChange(textInput)
        if !output.hostTextIsOwn { noteExternalChange() }
        updateAutocorrect()
        let shiftBefore = input.shift
        input.applyAutoCapitalization(autoCapitalization)
        if input.shift != shiftBefore { keyboardView.refreshLabels() }
        checkMemoryBudget()
        requestSuggestions()
        refreshTopBar()
    }

    override func didReceiveMemoryWarning() {
        super.didReceiveMemoryWarning()
        if gate.memoryWarning(now: ProcessInfo.processInfo.systemUptime) == .off {
            log.warning("Memory warning (\(Diagnostics.footprint())); prediction off for now")
            shedPrediction()
        } else {
            log.warning("Memory warning (\(Diagnostics.footprint())); prediction already off, cooldown restarted")
        }
    }

    // MARK: Input

    private func handle(_ event: GestureEvent) {
        let pageBefore = input.page, shiftBefore = input.shift
        output.beginEdit()
        switch event {
        case .tap(.nextKeyboard):
            advanceToNextInputMode()
            return
        case .tap(let action):
            input.handle(action, at: ProcessInfo.processInfo.systemUptime)
        case .commitAlternate(let text):
            input.insertAlternate(text)
        case .moveCursor(let offset):
            input.moveCursor(by: offset)
        case .deleteWord:
            input.deleteWordBackward()
        case .showAlternates, .highlightAlternate:
            return // Handled inside KeyboardView.
        case .openSettings:
            showSettings()
            return
        }
        afterInput(pageBefore: pageBefore, shiftBefore: shiftBefore)
    }

    private func afterInput(pageBefore: KeyboardPage, shiftBefore: ShiftState) {
        input.applyAutoCapitalization(autoCapitalization)
        if input.page != pageBefore {
            reloadKeys()
        } else if input.shift != shiftBefore {
            keyboardView.refreshLabels()
        }
        checkMemoryBudget()
        requestSuggestions()
        refreshTopBar()
        output.endEdit()
    }

    /// The cursor moved or the host changed the text: no undo, and no stale
    /// result may replace a word or stay in the bar.
    private func noteExternalChange() {
        input.textDidChangeExternally()
        latest = .empty
        lastRequest = nil
        barWord = nil
        output.resetEdits()
    }

    /// Autocorrect needs the user's setting, a host field that allows it, and a
    /// field that isn't for URLs, email addresses or web searches.
    private func updateAutocorrect() {
        let proxy = textDocumentProxy
        let fieldAllows: Bool
        switch proxy.keyboardType ?? .default {
        case .URL, .emailAddress, .webSearch: fieldAllows = false
        default: fieldAllows = proxy.autocorrectionType != .no
        }
        input.autocorrectEnabled = settings.autocorrect && fieldAllows
    }

    private var autoCapitalization: AutoCapitalization {
        switch textDocumentProxy.autocapitalizationType ?? .sentences {
        case .none: .off
        case .words: .words
        case .allCharacters: .allCharacters
        default: .sentences
        }
    }

    // MARK: Settings panel

    /// Covers the keys; touches already on them are dropped so lifting doesn't type.
    private func showSettings() {
        keyboardView.cancelAllTouches()
        settingsPanel.show(settings)
        settingsPanel.apply(theme)
        settingsPanel.isHidden = false
    }

    private func hideSettings() {
        settingsPanel.isHidden = true
    }

    // MARK: Appearance

    private func applySettings() {
        theme = KeyboardTheme.resolve(settings, style: traitCollection.userInterfaceStyle)
        view.backgroundColor = theme.background
        topBar.apply(theme)
        if !settingsPanel.isHidden { settingsPanel.apply(theme) }
        input.numberRow = settings.numberRow
        keyboardView.playsClicks = settings.keyClicks
        heightConstraint?.constant = settings.numberRow ? 320 : 270
        updateAutocorrect()
        lastRequest = nil // autocorrect may have changed: ask again
        requestSuggestions()
        reloadKeys()
        refreshTopBar()
    }

    private func reloadKeys() {
        keyboardView.configure(layout: input.layout, theme: theme) { [unowned self] key in
            input.label(for: key.action)
        }
    }

    /// After a Malayalam consonant: the best completion and the vowel signs,
    /// likeliest first. Otherwise the three suggestion slots.
    ///
    /// Only a result for the word now before the cursor is shown. While that
    /// result is on its way the bar stays as it is (a strip, or slots for this
    /// same word), so it isn't redrawn twice per key. Once the result or its
    /// timeout arrives without a match, the strip falls back to aksharamala
    /// order and the slots go empty; typing is unaffected either way.
    private func refreshTopBar() {
        let word = Self.straight(input.currentWord)
        let current = latest.isFor(word: word) ? latest : nil
        if input.language == .malayalam {
            let context = String((textDocumentProxy.documentContextBeforeInput ?? "").suffix(8))
            if let base = try? malayalamVowelSignBase(contextBefore: context) {
                // Vowel signs apply to whatever consonant is there, so an older strip can stay.
                if current == nil, awaitingResult, topBar.showsVowelSigns { return }
                barWord = word
                topBar.showVowelSigns(
                    base: base,
                    signs: Malayalam.orderedStripSigns(preferred: current?.vowelSigns ?? []),
                    completion: current?.candidates.first { $0.kind == .completion }
                )
                return
            }
        }
        if current == nil, awaitingResult, !topBar.showsVowelSigns, barWord == word { return }
        barWord = word
        let bar = SuggestionBar(current ?? .empty)
        topBar.showSuggestions(bar)
        noteBar(empty: bar.left == nil && bar.middle == nil && bar.right == nil, current: current)
    }

    /// Logs why the bar is empty (once per reason and word length) and when it fills again.
    /// Not while a result is on its way: the bar is briefly empty on every new word.
    private func noteBar(empty: Bool, current: SuggestionResult?) {
        guard empty else {
            if loggedEmptyBar != nil { log.info("Bar filled again") }
            loggedEmptyBar = nil
            return
        }
        if awaitingResult, suggestions.isEnabled { return }
        let reason: String
        if !suggestions.isEnabled {
            reason = gate.isOn ? "prediction off (resuming)" : "prediction off (memory gate)"
        } else if let lastOutcome, lastOutcome.outcome != .result {
            // Before the next check: a timeout's `.empty` also "matches" an empty word.
            reason = "\(lastOutcome.outcome.rawValue) after \(Diagnostics.milliseconds(lastOutcome.elapsed))"
        } else if current != nil {
            reason = input.currentWord.isEmpty ? "no next words" : "no candidates"
        } else if lastRequest == nil {
            reason = "no request since the text changed"
        } else {
            reason = "result was for another word (\(latest.typed.count) chars)"
        }
        let key = "\(reason)|\(input.currentWord.count)"
        guard key != loggedEmptyBar else { return }
        loggedEmptyBar = key
        log.info("Bar empty: \(reason); word \(input.currentWord.count) chars, \(input.language == .malayalam ? "ml" : "latin"), footprint \(Diagnostics.footprint())")
    }

    /// Types a short report into the field (no Full Access, so no pasteboard): the build,
    /// memory, prediction state and the newest diagnostics lines. No typed words.
    private func insertDiagnostics() {
        let footprint = MemoryMonitor.footprintBytes()
        peakFootprint = max(peakFootprint, footprint)
        let prediction = suggestions.isEnabled ? "on" : "off"
        let gateState = gate.isOn ? "on" : "off"
        let build = Diagnostics.buildDescription
        let system = "iOS \(UIDevice.current.systemVersion)"
        let peak = Diagnostics.megabytes(peakFootprint)
        log.info("Diagnostics inserted")
        hideSettings()
        Task { [weak self, provider] in
            let status = await provider.status()
            guard let self else { return }
            let header = "\(build); \(system); footprint \(Diagnostics.megabytes(footprint)) (peak \(peak)); gate \(gateState); prediction \(prediction); \(status)"
            textDocumentProxy.insertText(Diagnostics.report(header: header))
        }
    }

    private static func straight(_ text: String) -> String {
        text.replacingOccurrences(of: "\u{2019}", with: "'")
    }

    // MARK: Prediction and memory

    private func checkMemoryBudget() {
        let footprint = MemoryMonitor.footprintBytes()
        let now = ProcessInfo.processInfo.systemUptime
        peakFootprint = max(peakFootprint, footprint)
        if now - lastFootprintSample >= Self.footprintSampleInterval {
            lastFootprintSample = now
            log.info("Footprint \(Diagnostics.megabytes(footprint)) (peak \(Diagnostics.megabytes(peakFootprint))); gate \(gate.isOn ? "on" : "off")")
        }
        switch gate.update(footprintBytes: footprint, now: now) {
        case .off:
            log.warning("Over memory budget (\(Diagnostics.megabytes(footprint))); prediction off for now")
            shedPrediction()
        case .on:
            log.info("Memory recovered (\(Diagnostics.megabytes(footprint))); prediction back on")
            // Enabled only once the provider has resumed, so the first request isn't refused.
            Task { [weak self, provider] in
                await provider.resume()
                guard let self, gate.isOn else { return }
                suggestions.isEnabled = true
                lastRequest = nil // ask again for the word being typed
                requestSuggestions()
                refreshTopBar()
            }
        case nil:
            break
        }
    }

    /// Until the gate turns prediction back on.
    private func shedPrediction() {
        suggestions.isEnabled = false
        lastRequest = nil
        Task { [provider] in await provider.unload() }
    }

    private func requestSuggestions() {
        let request = SuggestionRequest(
            contextBefore: String((textDocumentProxy.documentContextBeforeInput ?? "").suffix(64)),
            language: input.language,
            autocorrect: input.autocorrectEnabled
        )
        guard request != lastRequest else { return }
        // A result for the other language must not stay in the bar.
        if request.language != lastRequest?.language { latest = .empty }
        if let last = lastRequest, last.language != request.language { barWord = nil }
        lastRequest = request
        suggestions.request(request)
        // A disabled coordinator never answers, so nothing is awaited.
        awaitingResult = suggestions.isEnabled
    }
}
