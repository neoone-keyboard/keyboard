import UIKit
import os
import KeyboardCore
import MLEngine

final class KeyboardViewController: UIInputViewController {
    private let log = Diagnostics(category: "keyboard")
    /// Largest footprint seen since the keyboard loaded, and when and at what it was last logged.
    private var peakFootprint: UInt64 = 0
    private var lastFootprintSample: Double = 0
    private var lastFootprintLogged: UInt64 = 0
    private static let footprintSampleInterval: Double = 5
    /// How the last delivered fetch ended, for explaining an empty bar.
    private var lastOutcome: (outcome: SuggestionCoordinator.Outcome, elapsed: Duration)?
    /// The empty-bar reason last logged, so a repeated render isn't logged twice.
    private var loggedEmptyBar: String?
    private var gate = PredictionGate(budget: .keyboardExtension)
    private let topBar = TopBarView()
    private let keyboardView = KeyboardView()
    private let settingsPanel = SettingsPanelView()
    /// In the top bar's place while open, with the keys below typing its search.
    private let yourWordsView = YourWordsView()
    /// Over the keys and the top bar while open; made on opening and dropped on closing, with
    /// its catalog.
    private var emojiPanel: EmojiPanelView?
    /// The panel's search: in the top bar's place while open, with the keys below typing it.
    private let emojiSearchView = EmojiSearchView()
    private var emojiSearchOpen: Bool { !emojiSearchView.isHidden }
    /// The recent emoji and the tone chosen for each, saved in the keyboard's container.
    private lazy var emojiHistory = EmojiHistory.load(from: Self.emojiHistoryURL)
    /// The top bar (or "Your words", or the emoji search) over the keys.
    private let stack = UIStackView()
    private var heightConstraint: NSLayoutConstraint?
    private var yourWordsHeight: NSLayoutConstraint?
    private var yourWordsOpen: Bool { !yourWordsView.isHidden }
    private var settings = KeyboardSettings()
    /// The suggestion count last given to the provider (`applySuggestionCount`).
    private var engineSuggestionCount: Int?
    private var theme = KeyboardTheme.resolve(KeyboardSettings(), style: .light)

    private let provider = EngineSuggestionProvider.shared
    private var lastRequest: SuggestionRequest?
    private var latest = SuggestionResult.empty
    /// A request was sent and neither its result nor its timeout has arrived.
    private var awaitingResult = false
    /// The word the bar's slots were made for (straight apostrophes). A tap on
    /// a slot made for another word is ignored.
    private var barWord: String?
    /// Counts swipes and keys: a swipe's words arriving after either are dropped.
    private var swipeGeneration = 0
    /// The last swipe gave no word: the bar says so until the next key.
    private var swipeFailed = false
    /// A swipe's words arriving later than this are dropped, and the bar asks for it again.
    private static let swipeTimeout: Duration = .milliseconds(250)

    private lazy var output = ProxyTextOutput { [unowned self] in self.textDocumentProxy }
    private lazy var input = InputController(output: output, languages: EngineSuggestionProvider.defaultLanguages)

    private lazy var suggestions = SuggestionCoordinator(provider: provider) { [weak self] result in
        guard let self else { return }
        awaitingResult = false
        latest = result
        if !result.isFor(word: Self.straight(input.currentWord)), lastOutcome?.outcome == .result {
            // Lengths only: no typed text in the diagnostics.
            log.info("Stale result: for a \(result.typed.count)-char word but the word is \(input.currentWord.count) chars; \(result.candidates.count) candidates dropped")
        }
        // The bar keeps a swipe's alternatives until the next key.
        if !input.showsSwipe { input.updateSuggestions(result) }
        refreshTopBar()
    }

    /// UIKit keeps every session's input view alive after its controller is released (iOS 26;
    /// `heap` on the Simulator finds one per session, the default UIInputView as well as ours),
    /// and with it whatever is still inside: the keys, the bar and the panel, about 1 MB a
    /// session until the memory gate turned prediction off. Emptied, and without its height
    /// constraint, what stays behind is about 50 KB a session (the app's `-keyboardCycles`).
    /// "Your words" and the emoji search are in `stack`, so they go with it.
    isolated deinit {
        emptyView()
        log.info("Keyboard released; footprint \(Diagnostics.footprint())")
    }

    /// Puts the keys, the bar and the settings panel in the input view: on loading, and again
    /// if a controller emptied by `emptyIdleControllers` is shown after all.
    private func attachViews() {
        view.addSubview(stack)
        view.addSubview(settingsPanel)
        NSLayoutConstraint.activate([
            heightConstraint,
            stack.leadingAnchor.constraint(equalTo: view.leadingAnchor, constant: 3),
            stack.trailingAnchor.constraint(equalTo: view.trailingAnchor, constant: -3),
            stack.topAnchor.constraint(equalTo: view.topAnchor, constant: 4),
            stack.bottomAnchor.constraint(equalTo: view.safeAreaLayoutGuide.bottomAnchor, constant: -2),
            settingsPanel.leadingAnchor.constraint(equalTo: keyboardView.leadingAnchor),
            settingsPanel.trailingAnchor.constraint(equalTo: keyboardView.trailingAnchor),
            settingsPanel.topAnchor.constraint(equalTo: keyboardView.topAnchor),
            settingsPanel.bottomAnchor.constraint(equalTo: keyboardView.bottomAnchor),
        ].compactMap { $0 })
    }

    /// Takes everything out of the input view (see `deinit`). A controller iOS keeps alive still
    /// owns its views, so the keys go too; `viewWillAppear` makes them again (`applySettings`).
    private func emptyView() {
        hideEmojiPanel()
        keyboardView.removeKeys()
        stack.removeFromSuperview()
        settingsPanel.removeFromSuperview()
        heightConstraint?.isActive = false
    }

    /// The controllers alive in this process. iOS 27 sometimes loads a controller, often just
    /// as another session closes, never shows it and keeps it: its deinit never runs, and its
    /// keys and bar stayed in memory, a few MB each, until the memory gate turned prediction
    /// off for good (iPhone 15 Pro Max: 76 sessions, 5 never shown and 5 never released).
    private static var alive: [Held] = []
    private struct Held { weak var controller: KeyboardViewController? }

    /// Empties every earlier controller that isn't on screen, as its deinit would; one that is
    /// shown after all puts its views back (`viewWillAppear`). Returns how many it emptied.
    private func emptyIdleControllers() -> Int {
        Self.alive.removeAll { $0.controller == nil }
        let idle = Self.alive.compactMap(\.controller)
            .filter { $0 !== self && $0.viewIfLoaded?.window == nil && $0.stack.superview != nil }
        for controller in idle { controller.emptyView() }
        Self.alive.append(Held(controller: self))
        return idle.count
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
        keyboardView.onPress = { [unowned self] in prefetchSuggestions(after: $0) }
        topBar.onVowelSign = { [unowned self] sign in
            let keys = input.keysShown, shift = input.shift
            output.beginEdit()
            input.handle(.character(sign), at: ProcessInfo.processInfo.systemUptime)
            afterInput(keysBefore: keys, shiftBefore: shift)
        }
        topBar.onSuggestion = { [unowned self] slot in
            // The bar may still show another word's slots while a result is on its way.
            guard barWord == Self.straight(input.currentWord) else { return }
            let keys = input.keysShown, shift = input.shift
            output.beginEdit()
            if slot.suggestion.kind == .emoji {
                input.commitEmojiSuggestion(slot.suggestion.text)
                noteEmojiUsed(slot.suggestion.text)
            } else {
                input.commitSuggestion(slot)
            }
            afterInput(keysBefore: keys, shiftBefore: shift)
        }
        // Removal runs on the provider's actor; the bar asks again once it is done, so the
        // word is gone from it and the next best takes its slot.
        topBar.onRemoveSuggestion = { [unowned self] word in
            // Lengths only: no typed text in the diagnostics.
            log.info("Suggestion removed; \(word.count) chars")
            Task { [weak self, provider] in
                await provider.remove(word)
                guard let self else { return }
                refreshLearnedAddresses()
                lastRequest = nil
                requestSuggestions()
                refreshTopBar()
            }
        }
        // Learning runs on the provider's actor; the main thread never waits for it.
        input.onLearningEvent = { [weak self, provider] event in
            Task {
                await provider.handle(event)
                self?.refreshLearnedAddresses()
            }
        }
        input.onSwipeOutcome = { [provider] outcome in
            Task { await provider.noteSwipeOutcome(outcome) }
        }
        // A use with a digit may be the end of a phone number, which the engine learns from it.
        input.onWordUsed = { [weak self, provider] use in
            Task {
                await provider.noteUse(use)
                if use.word.contains(where: \.isNumber) { self?.refreshLearnedAddresses() }
            }
        }

        settingsPanel.onChange = { [unowned self] changed in
            settings = changed
            applySettings()
        }
        settingsPanel.onDone = { [unowned self] in hideSettings() }
        settingsPanel.onInsertDiagnostics = { [unowned self] in insertDiagnostics() }
        // The actor clears the log and the in-memory store; the main thread doesn't wait.
        settingsPanel.onForgetLearned = { [weak self, provider] in
            self?.forgetEmojiHistory()
            Task {
                await provider.forgetLearned()
                self?.refreshLearnedAddresses()
            }
        }

        settingsPanel.onYourWords = { [unowned self] in showYourWords() }
        settingsPanel.onLanguagesChange = { [unowned self] choice in chooseLanguages(choice) }
        yourWordsView.onBack = { [unowned self] in
            hideYourWords()
            showSettings()
        }
        yourWordsView.onForget = { [unowned self] word in
            // Lengths only: no typed text in the diagnostics.
            log.info("Learned word forgotten; \(word.count) chars")
            changeUserWords { await $0.forget(word) }
        }
        yourWordsView.onRestore = { [unowned self] word in
            log.info("Removed word restored; \(word.count) chars")
            changeUserWords { await $0.restore(word) }
        }

        stack.addArrangedSubview(topBar)
        stack.addArrangedSubview(yourWordsView)
        stack.addArrangedSubview(emojiSearchView)
        stack.addArrangedSubview(keyboardView)
        yourWordsView.isHidden = true
        emojiSearchView.isHidden = true
        emojiSearchView.onBack = { [unowned self] in
            hideEmojiSearch()
            showEmojiPanel()
        }
        emojiSearchView.onPick = { [unowned self] emoji in insertPickedEmoji(emoji, toneFor: nil) }
        stack.axis = .vertical
        stack.spacing = 4
        stack.translatesAutoresizingMaskIntoConstraints = false
        settingsPanel.translatesAutoresizingMaskIntoConstraints = false
        settingsPanel.isHidden = true

        let height = view.heightAnchor.constraint(equalToConstant: 270)
        height.priority = .defaultHigh + 1
        heightConstraint = height
        // Below required: the stack collapses whichever of the two is hidden with its own
        // required constraint, which would otherwise conflict.
        let wordsHeight = yourWordsView.heightAnchor.constraint(equalToConstant: Self.topBarHeight)
        wordsHeight.priority = .required - 1
        yourWordsHeight = wordsHeight
        let searchHeight = emojiSearchView.heightAnchor.constraint(
            equalToConstant: Self.topBarHeight + EmojiSearchView.extraHeight)
        searchHeight.priority = .required - 1
        let barHeight = topBar.heightAnchor.constraint(equalToConstant: Self.topBarHeight)
        barHeight.priority = .required - 1
        NSLayoutConstraint.activate([wordsHeight, searchHeight, barHeight])
        attachViews()

        registerForTraitChanges([UITraitUserInterfaceStyle.self]) { (self: Self, _: UITraitCollection) in
            self.applySettings()
        }
        // "Your words" grows the keyboard less in landscape.
        registerForTraitChanges([UITraitVerticalSizeClass.self]) { (self: Self, _: UITraitCollection) in
            self.updateHeight()
        }
        Self.lastLoad = .now
        Self.sessions += 1
        let emptied = emptyIdleControllers()
        log.info("Keyboard loaded; \(Diagnostics.buildDescription); iOS \(UIDevice.current.systemVersion); "
                 + "\(Self.alive.count) controllers alive, \(emptied) off screen emptied; footprint \(Diagnostics.footprint())")
    }

    override func viewWillAppear(_ animated: Bool) {
        super.viewWillAppear(animated)
        if stack.superview == nil {
            attachViews()
            log.info("Emptied keyboard shown after all; views put back")
        }
        settings = KeyboardSettings.load(from: .standard)
        hideYourWords()
        hideSettings()
        hideEmojiPanel()
        hideEmojiSearch()
        noteExternalChange()
        applySettings()
    }

    /// The keys are showing: open the lexicons now, off the main thread, so the
    /// first word typed already has suggestions, and give the LM the text already there.
    override func viewDidAppear(_ animated: Bool) {
        super.viewDidAppear(animated)
        // The field's keyboard type (UIKeyboardType raw value: 4 number pad, 5 phone pad, 8 decimal pad,
        // 11 ASCII number pad), to see which numeric fields iOS hands to this keyboard.
        log.info("Appeared; prediction \(suggestions.isEnabled ? "on" : "off"); field type \(textDocumentProxy.keyboardType?.rawValue ?? -1); footprint \(Diagnostics.footprint())")
        guard suggestions.isEnabled else { return }
        // Read on the main thread, before the hop to the provider.
        let contextBefore = textDocumentProxy.documentContextBeforeInput ?? ""
        // The provider is shared: an earlier session may have shed it for memory.
        Task { [weak self, provider] in
            await provider.resume()
            await provider.reloadIfChanged()
            await provider.warmUp(contextBefore: contextBefore)
            let layouts = await provider.layouts()
            self?.refreshLearnedAddresses()
            if let layouts { self?.applyLayouts(layouts) }
        }
    }

    /// The lists of "Your words", the emoji panel and its search are held only while they show.
    override func viewDidDisappear(_ animated: Bool) {
        super.viewDidDisappear(animated)
        hideYourWords()
        hideEmojiPanel()
        hideEmojiSearch()
        Self.restartIfBloated(provider: provider)
    }

    /// iOS keeps part of every keyboard session alive in the process, and nothing here can free
    /// it: TextInputUI's input manager keeps each session's text proxy, and UIKit each session's
    /// input view with its layout engine and constraints (`heap` on the Simulator: one of each
    /// per session, about 120 KB; the phone's footprint at load rose about 0.3 MB a session until
    /// the memory gate turned prediction off). So once the footprint has grown past this, the
    /// process ends itself while no keyboard shows, and iOS starts a fresh one when it is needed.
    private static let restartFootprint: UInt64 = 24 * 1_048_576
    /// How long nothing must show, and no controller have loaded, before that.
    private static let restartDelay: Duration = .seconds(5)
    private static var lastLoad = ContinuousClock.now
    private static var sessions = 0

    private static func restartIfBloated(provider: EngineSuggestionProvider) {
        Task { @MainActor in
            try? await Task.sleep(for: restartDelay)
            func idle() -> Bool {
                ContinuousClock.now - lastLoad >= restartDelay
                    && alive.allSatisfy { $0.controller?.viewIfLoaded?.window == nil }
            }
            let footprint = MemoryMonitor.footprintBytes()
            guard idle(), footprint >= restartFootprint else { return }
            // Learning queued on the engine's actor before this is written by now.
            await provider.idle()
            guard idle() else { return }
            Diagnostics(category: "keyboard").info(
                "Restarting to free what iOS keeps of past sessions; footprint "
                    + "\(Diagnostics.megabytes(footprint)) after \(sessions) sessions"
            )
            Diagnostics.file?.flush()
            exit(0)
        }
    }

    /// Gives the input controller the engine's learned addresses, so letters that begin one
    /// are one word there too. Read off the main thread; a list a key or two stale is fine.
    private func refreshLearnedAddresses() {
        Task { [weak self, provider] in
            let addresses = await provider.learnedAddresses()
            self?.input.learnedAddresses = addresses
        }
    }

    /// The language key cycles the engine's layouts (its active languages). Until the engine
    /// opens it cycles `EngineSuggestionProvider.defaultLanguages`, the same list by default,
    /// so this redraws only when the engine's list differs.
    private func applyLayouts(_ layouts: [InputLayout]) {
        applyLanguages(EngineSuggestionProvider.languages(for: layouts))
    }

    /// A language turned on or off in the settings panel: the language key cycles the new list
    /// at once (on the first language if the current one went off), and the bar follows.
    private func chooseLanguages(_ choice: ActiveLanguages) {
        log.info("Languages chosen in the keyboard: \(choice.languages.joined(separator: ","))")
        Task { [weak self, provider] in
            let layouts = await provider.choose(choice)
            guard let self else { return }
            if let layouts {
                applyLayouts(layouts)
            } else {
                applyLanguages(EngineSuggestionProvider.defaultLanguages)
            }
        }
    }

    private func applyLanguages(_ languages: [Language]) {
        guard !languages.isEmpty, languages != input.languages else { return }
        let before = input.language.id
        input.languages = languages
        log.info("Languages now \(languages.map(\.id).joined(separator: ",")); was on \(before), now \(input.language.id)")
        // The keys, their labels and the bar may all change with the list.
        applySettings()
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
        topBar.cancelRemoval()
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
        emojiPanel?.releaseMemory()
        emojiSearchView.releaseMemory()
        if gate.memoryWarning(now: ProcessInfo.processInfo.systemUptime) == .off {
            log.warning("Memory warning (\(Diagnostics.footprint())); prediction off for now")
            shedPrediction()
        } else {
            log.warning("Memory warning (\(Diagnostics.footprint())); prediction already off, cooldown restarted")
        }
    }

    // MARK: Input

    private func handle(_ event: GestureEvent) {
        if yourWordsOpen {
            handleSearch(event)
            return
        }
        if emojiSearchOpen {
            handleEmojiSearch(event)
            return
        }
        swipeGeneration += 1
        swipeFailed = false
        if case .swipe(let points) = event {
            swipe(points)
            return
        }
        let keysBefore = input.keysShown, shiftBefore = input.shift
        output.beginEdit()
        switch event {
        case .tap(.nextKeyboard):
            advanceToNextInputMode()
            return
        case .tap(.emoji):
            showEmojiPanel()
            return
        case .tap(let action):
            input.handle(action, at: ProcessInfo.processInfo.systemUptime)
        case .commitAlternate(let text):
            input.insertAlternate(text)
        case .moveCursor(let offset):
            input.moveCursor(by: offset)
        case .deleteWord:
            input.deleteWordBackward()
        case .showAlternates, .highlightAlternate, .swipe:
            return // Handled inside KeyboardView, or above.
        case .openSettings:
            showSettings()
            return
        }
        afterInput(keysBefore: keysBefore, shiftBefore: shiftBefore)
    }

    /// Decodes a swipe on the provider's actor and types its best word, with the others in the
    /// bar. Without prediction, or with no word for the path, the bar asks for it again.
    private func swipe(_ points: [CGPoint]) {
        let generation = swipeGeneration
        guard suggestions.isEnabled, input.language == .english, !input.secureTextEntry else {
            showSwipeFailed()
            return
        }
        let path = points.map { SwipePoint(x: Float($0.x), y: Float($0.y)) }
        let keys = keyboardView.letterKeys.map {
            SwipeKey(letter: $0.letter, x: Float($0.rect.minX), y: Float($0.rect.minY),
                     width: Float($0.rect.width), height: Float($0.rect.height))
        }
        // The word goes in after a space (`InputController.insertSwipe`).
        var context = String((textDocumentProxy.documentContextBeforeInput ?? "").suffix(64))
        if let last = context.last, !last.isWhitespace { context += " " }
        let started = ContinuousClock.now
        Task { [weak self, provider] in
            let found = await provider.swipe(path, keys: keys, contextBefore: context)
            guard let self, generation == swipeGeneration else { return }
            let elapsed = ContinuousClock.now - started
            guard let found, let best = found.first, elapsed <= Self.swipeTimeout else {
                if found != nil { log.warning("Swipe dropped: its words came after \(Diagnostics.milliseconds(elapsed))") }
                showSwipeFailed()
                return
            }
            let shown = input.keysShown, shift = input.shift
            output.beginEdit()
            input.insertSwipe(best, alternatives: Array(found.dropFirst().prefix(2)))
            afterInput(keysBefore: shown, shiftBefore: shift)
        }
    }

    private func showSwipeFailed() {
        swipeFailed = true
        refreshTopBar()
    }

    private func afterInput(keysBefore: KeysShown, shiftBefore: ShiftState) {
        // Typing answers "Remove …?" with no.
        topBar.cancelRemoval()
        input.applyAutoCapitalization(autoCapitalization)
        if input.keysShown != keysBefore {
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
        swipeFailed = false
        swipeGeneration += 1
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
        input.secureTextEntry = proxy.isSecureTextEntry ?? false
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
        topBar.cancelRemoval()
        settingsPanel.show(settings, available: EngineSuggestionProvider.availableLanguages,
                           active: EngineSuggestionProvider.activeChoice)
        settingsPanel.apply(theme)
        settingsPanel.isHidden = false
    }

    private func hideSettings() {
        settingsPanel.isHidden = true
    }

    // MARK: Emoji

    private static let emojiHistoryURL = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
        .appending(path: "Emoji/history.json")
    /// Saves run in order, off the main thread.
    private static let emojiSaves = DispatchQueue(label: "me.jishnu.keyboard.emoji-history")
    /// The word → emoji list, memory-mapped once per process; nil if missing from the bundle.
    private static let emojiWords: EmojiWords? = Bundle.main
        .url(forResource: "emoji-words", withExtension: "txt", subdirectory: "Emoji")
        .flatMap { try? Data(contentsOf: $0, options: .alwaysMapped) }
        .map(EmojiWords.init)

    /// The panel's search words, memory-mapped once per process; nil if missing from the bundle.
    private static let emojiSearchWords: EmojiWords? = Bundle.main
        .url(forResource: "emoji-search", withExtension: "txt", subdirectory: "Emoji")
        .flatMap { try? Data(contentsOf: $0, options: .alwaysMapped) }
        .map(EmojiWords.init)

    /// Covers the keys and the top bar, with the catalog read from the bundle.
    private func showEmojiPanel() {
        let start = ContinuousClock.now
        guard emojiPanel == nil,
              let url = Bundle.main.url(forResource: "emoji", withExtension: "txt", subdirectory: "Emoji"),
              let text = try? String(contentsOf: url, encoding: .utf8)
        else { return }
        keyboardView.cancelAllTouches()
        topBar.cancelRemoval()
        let before = MemoryMonitor.footprintBytes()
        let panel = EmojiPanelView()
        panel.onEmoji = { [unowned self] emoji, base in insertPickedEmoji(emoji, toneFor: base) }
        panel.onBackspace = { [unowned self] in
            let keys = input.keysShown, shift = input.shift
            output.beginEdit()
            input.deleteBackwardWholeEmoji()
            afterInput(keysBefore: keys, shiftBefore: shift)
        }
        panel.onLetters = { [unowned self] in hideEmojiPanel() }
        panel.onSearch = { [unowned self] in showEmojiSearch() }
        panel.translatesAutoresizingMaskIntoConstraints = false
        view.addSubview(panel)
        // Its search field takes the top bar's place.
        NSLayoutConstraint.activate([
            panel.leadingAnchor.constraint(equalTo: keyboardView.leadingAnchor),
            panel.trailingAnchor.constraint(equalTo: keyboardView.trailingAnchor),
            panel.topAnchor.constraint(equalTo: stack.topAnchor),
            panel.bottomAnchor.constraint(equalTo: keyboardView.bottomAnchor),
        ])
        panel.apply(theme)
        panel.show(catalog: EmojiCatalog(text: text), history: emojiHistory, lettersTitle: input.label(for: .page(.letters)))
        emojiPanel = panel
        log.info("Emoji panel opened in \(Diagnostics.milliseconds(.now - start)); footprint \(Diagnostics.footprint()) "
                 + "(\(Diagnostics.megabytes(before)) before)")
    }

    private func hideEmojiPanel() {
        guard let panel = emojiPanel else { return }
        let statistics = panel.statistics
        panel.removeFromSuperview()
        emojiPanel = nil
        log.info("Emoji panel closed; \(statistics); \(Diagnostics.footprint()) once dropped")
        // What the panel left behind once its views, pictures and fonts have had time to go.
        DispatchQueue.main.asyncAfter(deadline: .now() + 2) { [log, weak panel] in
            log.info("Emoji panel closed 2 s ago; footprint \(Diagnostics.footprint()), panel "
                     + "\(panel == nil ? "freed" : "STILL ALIVE"), \(EmojiRenderer.fontCopiesAlive) private fonts alive")
        }
    }

    /// An emoji picked in the panel or its search, with the base it is a chosen tone for.
    private func insertPickedEmoji(_ emoji: String, toneFor base: String?) {
        if let base { emojiHistory.chooseTone(emoji, for: base) }
        let keys = input.keysShown, shift = input.shift
        output.beginEdit()
        input.insertEmoji(emoji)
        noteEmojiUsed(emoji)
        afterInput(keysBefore: keys, shiftBefore: shift)
    }

    /// Swaps the panel for its search, in the top bar's place, over the letter keys. The
    /// panel's catalog goes to the search; the panel itself is dropped.
    private func showEmojiSearch() {
        guard let catalog = emojiPanel?.catalog, let words = Self.emojiSearchWords else { return }
        hideEmojiPanel()
        keyboardView.cancelAllTouches()
        if input.page != .letters, !input.page.isNative {
            input.handle(.page(.letters), at: ProcessInfo.processInfo.systemUptime)
            reloadKeys()
        }
        emojiSearchView.apply(theme)
        emojiSearchView.show(EmojiSearch(words: words, catalog: catalog, history: emojiHistory))
        topBar.isHidden = true
        emojiSearchView.isHidden = false
        updateHeight()
        log.info("Emoji search opened; footprint \(Diagnostics.footprint())")
    }

    /// Back to the top bar; the search and its pictures are dropped.
    private func hideEmojiSearch() {
        guard emojiSearchOpen else { return }
        emojiSearchView.isHidden = true
        emojiSearchView.release()
        topBar.isHidden = false
        updateHeight()
        requestSuggestions()
        refreshTopBar()
        log.info("Emoji search closed; footprint \(Diagnostics.footprint())")
    }

    /// While the emoji search shows, letters, space and backspace edit its query, as with
    /// "Your words"; return and the emoji key go back to the panel's grid.
    private func handleEmojiSearch(_ event: GestureEvent) {
        switch event {
        case .tap(let action):
            switch emojiSearchView.handle(action) {
            case .edited, .ignored:
                return
            case .close:
                hideEmojiSearch()
                showEmojiPanel()
            case .passThrough:
                if action == .nextKeyboard {
                    advanceToNextInputMode()
                    return
                }
                let keysBefore = input.keysShown, shiftBefore = input.shift
                input.handle(action, at: ProcessInfo.processInfo.systemUptime)
                if input.keysShown != keysBefore {
                    reloadKeys()
                } else if input.shift != shiftBefore {
                    keyboardView.refreshLabels()
                }
            }
        case .commitAlternate(let text):
            emojiSearchView.type(text)
        case .deleteWord:
            emojiSearchView.clearQuery()
        case .openSettings:
            hideEmojiSearch()
            showSettings()
        case .moveCursor, .showAlternates, .highlightAlternate, .swipe:
            break
        }
    }

    /// An emoji went in, from the panel or the bar: first in the recents, saved at once.
    private func noteEmojiUsed(_ emoji: String) {
        emojiHistory.record(emoji)
        let history = emojiHistory, url = Self.emojiHistoryURL
        Self.emojiSaves.async { try? history.save(to: url) }
    }

    /// "Forget learned words" forgets the recent emoji and tones too.
    private func forgetEmojiHistory() {
        emojiHistory = EmojiHistory()
        let url = Self.emojiHistoryURL
        Self.emojiSaves.async { try? EmojiHistory.forget(at: url) }
    }

    /// The emoji for the bar's right slot, if the setting is on, the field isn't for a
    /// password, a URL or an email address, and the word has one.
    private func barEmoji() -> String? {
        let proxy = textDocumentProxy
        guard settings.emojiSuggestions, !input.secureTextEntry, let words = Self.emojiWords,
              proxy.keyboardType != .URL, proxy.keyboardType != .emailAddress,
              let word = EmojiSuggestion.word(contextBefore: proxy.documentContextBeforeInput ?? "", currentWord: input.currentWord)
        else { return nil }
        return EmojiSuggestion.pick(words.emoji(for: word), history: emojiHistory)
    }

    // MARK: Your words

    private static let topBarHeight: CGFloat = 40

    /// How much taller the keyboard grows for "Your words" (it takes the top bar's place and
    /// this): a short list, so the keys stay showing to type its search. Less in landscape.
    private var yourWordsExtraHeight: CGFloat {
        traitCollection.verticalSizeClass == .compact ? 60 : 160
    }

    /// Replaces the settings panel and the top bar, and reads the lists off the main thread.
    private func showYourWords() {
        hideSettings()
        keyboardView.cancelAllTouches()
        topBar.cancelRemoval()
        yourWordsView.apply(theme)
        yourWordsView.show(nil)
        topBar.isHidden = true
        yourWordsView.isHidden = false
        updateHeight()
        Task { [weak self, provider] in
            let words = await provider.yourWords()
            guard let self, yourWordsOpen else { return }
            log.info("Your words opened; \(words.learned.count) learned, \(words.removed.count) removed; footprint \(Diagnostics.footprint())")
            yourWordsView.show(words)
        }
    }

    /// Back to the top bar; the lists are dropped. The language may have changed meanwhile,
    /// so the bar asks again.
    private func hideYourWords() {
        guard yourWordsOpen else { return }
        yourWordsView.isHidden = true
        yourWordsView.release()
        topBar.isHidden = false
        updateHeight()
        requestSuggestions()
        refreshTopBar()
    }

    /// While "Your words" shows, letters and backspace edit its search, and the host's text
    /// is left alone. Keys that change the keys (a page, the language) still do, through the
    /// input controller, which writes no text for them.
    private func handleSearch(_ event: GestureEvent) {
        switch event {
        case .tap(let action):
            guard yourWordsView.handle(action) == .passThrough else { return }
            if action == .nextKeyboard {
                advanceToNextInputMode()
                return
            }
            let keysBefore = input.keysShown, shiftBefore = input.shift
            input.handle(action, at: ProcessInfo.processInfo.systemUptime)
            if input.keysShown != keysBefore {
                reloadKeys()
            } else if input.shift != shiftBefore {
                keyboardView.refreshLabels()
            }
        case .commitAlternate(let text):
            yourWordsView.type(text)
        case .deleteWord:
            yourWordsView.clearQuery()
        case .openSettings:
            hideYourWords()
            showSettings()
        case .moveCursor, .showAlternates, .highlightAlternate, .swipe:
            break
        }
    }

    /// Runs a forget or restore on the provider's actor, then asks the bar again, as removal
    /// does, so the change shows in the next suggestions.
    private func changeUserWords(_ change: @escaping @Sendable (EngineSuggestionProvider) async -> Void) {
        Task { [weak self, provider] in
            await change(provider)
            guard let self else { return }
            refreshLearnedAddresses()
            lastRequest = nil
            requestSuggestions()
            refreshTopBar()
        }
    }

    private func updateHeight() {
        let extra = yourWordsOpen ? yourWordsExtraHeight : emojiSearchOpen ? EmojiSearchView.extraHeight : 0
        yourWordsHeight?.constant = Self.topBarHeight + extra
        heightConstraint?.constant = (settings.numberRow ? 320 : 270) + extra
    }

    // MARK: Appearance

    private func applySettings() {
        theme = KeyboardTheme.resolve(settings, style: traitCollection.userInterfaceStyle)
        view.backgroundColor = theme.background
        topBar.apply(theme)
        if !settingsPanel.isHidden { settingsPanel.apply(theme) }
        if yourWordsOpen { yourWordsView.apply(theme) }
        if emojiSearchOpen { emojiSearchView.apply(theme) }
        input.numberRow = settings.numberRow
        keyboardView.playsClicks = settings.keyClicks
        updateHeight()
        updateAutocorrect()
        applySuggestionCount()
        lastRequest = nil // autocorrect may have changed: ask again
        requestSuggestions()
        reloadKeys()
        refreshTopBar()
    }

    /// The engine returns as many suggestions as the bar holds. A change is set on the
    /// provider first, then the bar asks again, so the new count shows at once.
    private func applySuggestionCount() {
        let count = settings.suggestionCount
        guard count != engineSuggestionCount else { return }
        engineSuggestionCount = count
        Task { [weak self, provider] in
            await provider.setMaxSuggestions(count)
            guard let self, self.engineSuggestionCount == count else { return }
            lastRequest = nil
            requestSuggestions()
        }
    }

    private func reloadKeys() {
        keyboardView.configure(layout: input.layout, theme: theme) { [unowned self] key in
            input.label(for: key.action)
        }
    }

    /// After a Malayalam consonant: the best completion and the vowel signs,
    /// likeliest first. Otherwise the three suggestion slots, with an emoji for the
    /// word in the right one when there is one (`barEmoji`).
    ///
    /// Only a result for the word now before the cursor is shown. While that
    /// result is on its way the bar stays as it is (a strip, or slots for this
    /// same word), so it isn't redrawn twice per key. Once the result or its
    /// timeout arrives without a match, the strip falls back to aksharamala
    /// order and the slots go empty; typing is unaffected either way.
    private func refreshTopBar() {
        if swipeFailed {
            topBar.showMessage("Swipe again")
            return
        }
        let word = Self.straight(input.currentWord)
        if input.showsSwipe {
            barWord = word
            topBar.showSuggestions(SuggestionBar(input.suggestions, limit: settings.suggestionCount))
            return
        }
        let current = latest.isFor(word: word) ? latest : nil
        if case .pack(let pack) = input.language, let layout = pack.layout, !layout.vowelSignStrip.isEmpty {
            let context = String((textDocumentProxy.documentContextBeforeInput ?? "").suffix(8))
            if let base = try? nativeMarkBase(script: pack.script, contextBefore: context) {
                // Vowel signs apply to whatever consonant is there, so an older strip can stay.
                if current == nil, awaitingResult, topBar.showsVowelSigns { return }
                barWord = word
                topBar.showVowelSigns(
                    base: base,
                    signs: layout.orderedStripSigns(preferred: current?.vowelSigns ?? []),
                    completion: current?.candidates.first { $0.kind == .completion }
                )
                return
            }
        }
        if current == nil, awaitingResult, !topBar.showsVowelSigns, barWord == word { return }
        barWord = word
        let bar = SuggestionBar(current ?? .empty, limit: settings.suggestionCount)
        topBar.showSuggestions(bar.withEmoji(barEmoji()))
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
        // Once per reason until the bar fills: not once per key while prediction is off.
        guard reason != loggedEmptyBar else { return }
        loggedEmptyBar = reason
        log.info("Bar empty: \(reason); word \(input.currentWord.count) chars, \(input.language.id), footprint \(Diagnostics.footprint())")
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
        // Only when it moved by a megabyte or more, so a steady footprint isn't logged every 5 s.
        let moved = footprint.distance(to: lastFootprintLogged).magnitude >= 1_048_576
        if now - lastFootprintSample >= Self.footprintSampleInterval, moved || lastFootprintLogged == 0 {
            lastFootprintSample = now
            lastFootprintLogged = footprint
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
        // The bar keeps a swipe's alternatives until the next key.
        guard !input.showsSwipe else { return }
        let request = suggestionRequest(contextBefore: textDocumentProxy.documentContextBeforeInput ?? "")
        guard request != lastRequest else { return }
        // A result for the other language must not stay in the bar.
        if request.language != lastRequest?.language { latest = .empty }
        if let last = lastRequest, last.language != request.language { barWord = nil }
        lastRequest = request
        suggestions.request(request)
        // A disabled coordinator never answers, so nothing is awaited.
        awaitingResult = suggestions.isEnabled
    }

    private func suggestionRequest(contextBefore: String) -> SuggestionRequest {
        SuggestionRequest(
            contextBefore: String(contextBefore.suffix(64)),
            language: input.language,
            autocorrect: input.autocorrectEnabled
        )
    }

    /// A finger is on a letter: the engine starts on the suggestions its key-up will ask for
    /// (`EngineSuggestionProvider.prefetch`). The key-up still makes its own request.
    /// On space it only brings the LM's context up to the finished word: the key-up reports
    /// the word's use, which would drop a result made ahead, but the LM's context is kept.
    private func prefetchSuggestions(after action: KeyAction) {
        guard suggestions.isEnabled, !yourWordsOpen, !emojiSearchOpen, emojiPanel == nil else { return }
        if action == .space {
            let context = String(input.contextAfterSpace().suffix(64))
            Task { [provider] in await provider.warmUp(contextBefore: context) }
            return
        }
        guard let text = input.insertion(for: action) else { return }
        let request = suggestionRequest(contextBefore: (textDocumentProxy.documentContextBeforeInput ?? "") + text)
        Task { [provider] in await provider.prefetch(request) }
    }
}
