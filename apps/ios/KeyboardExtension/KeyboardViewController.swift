import UIKit
import os
import KeyboardCore
import MLEngine

final class KeyboardViewController: UIInputViewController {
    private let log = Logger(subsystem: "me.jishnu.keyboard.extension", category: "keyboard")
    private let budget = MemoryBudget.keyboardExtension
    private let topBar = TopBarView()
    private let keyboardView = KeyboardView()
    private var heightConstraint: NSLayoutConstraint?
    private var settings = KeyboardSettings()
    private var theme = KeyboardTheme.resolve(KeyboardSettings(), style: .light)

    private let provider = EngineSuggestionProvider()
    private var lastRequestedContext: String?
    private var latestSuggestions: [String] = []

    private lazy var input = InputController(output: ProxyTextOutput { [unowned self] in
        self.textDocumentProxy
    })

    private lazy var suggestions = SuggestionCoordinator(provider: provider) { [weak self] results in
        self?.latestSuggestions = results
        self?.refreshTopBar()
    }

    override func viewDidLoad() {
        super.viewDidLoad()
        keyboardView.onEvent = { [unowned self] in handle($0) }
        topBar.onVowelSign = { [unowned self] sign in
            let page = input.page, shift = input.shift
            input.handle(.character(sign), at: ProcessInfo.processInfo.systemUptime)
            afterInput(pageBefore: page, shiftBefore: shift)
        }

        let stack = UIStackView(arrangedSubviews: [topBar, keyboardView])
        stack.axis = .vertical
        stack.spacing = 4
        stack.translatesAutoresizingMaskIntoConstraints = false
        view.addSubview(stack)

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
        ])

        registerForTraitChanges([UITraitUserInterfaceStyle.self]) { (self: Self, _: UITraitCollection) in
            self.applySettings()
        }
        log.info("Keyboard loaded; footprint \(MemoryMonitor.footprintBytes(), privacy: .public) bytes")
    }

    override func viewWillAppear(_ animated: Bool) {
        super.viewWillAppear(animated)
        settings = KeyboardSettings.sharedDefaults.map(KeyboardSettings.load(from:)) ?? KeyboardSettings()
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
        let shiftBefore = input.shift
        input.applyAutoCapitalization(autoCapitalization)
        if input.shift != shiftBefore { keyboardView.refreshLabels() }
        checkMemoryBudget()
        requestSuggestions()
        refreshTopBar()
    }

    override func didReceiveMemoryWarning() {
        super.didReceiveMemoryWarning()
        log.warning("Memory warning; prediction off")
        shedPrediction()
    }

    // MARK: Input

    private func handle(_ event: GestureEvent) {
        let pageBefore = input.page, shiftBefore = input.shift
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
    }

    private var autoCapitalization: AutoCapitalization {
        switch textDocumentProxy.autocapitalizationType ?? .sentences {
        case .none: .off
        case .words: .words
        case .allCharacters: .allCharacters
        default: .sentences
        }
    }

    // MARK: Appearance

    private func applySettings() {
        theme = KeyboardTheme.resolve(settings, style: traitCollection.userInterfaceStyle)
        view.backgroundColor = theme.background
        topBar.apply(theme)
        input.numberRow = settings.numberRow
        keyboardView.playsClicks = settings.keyClicks
        heightConstraint?.constant = settings.numberRow ? 320 : 270
        reloadKeys()
        refreshTopBar()
    }

    private func reloadKeys() {
        keyboardView.configure(layout: input.layout, theme: theme) { [unowned self] key in
            input.label(for: key.action)
        }
    }

    /// Malayalam: vowel signs when the text ends in a consonant. English: suggestions.
    /// If the Rust call fails the strip simply doesn't appear; typing is unaffected.
    private func refreshTopBar() {
        if input.language == .malayalam {
            let context = String((textDocumentProxy.documentContextBeforeInput ?? "").suffix(8))
            if let base = try? malayalamVowelSignBase(contextBefore: context) {
                topBar.showVowelSigns(base: base, signs: Malayalam.stripSigns)
            } else {
                topBar.showSuggestions([])
            }
            return
        }
        topBar.showSuggestions(latestSuggestions)
    }

    // MARK: Prediction and memory

    private func checkMemoryBudget() {
        guard suggestions.isEnabled else { return }
        if budget.pressure(footprintBytes: MemoryMonitor.footprintBytes()) == .shed {
            log.warning("Over memory budget; prediction off")
            shedPrediction()
        }
    }

    /// Permanent until the system recreates the extension: prediction is never re-enabled.
    private func shedPrediction() {
        suggestions.isEnabled = false
        lastRequestedContext = nil
        Task { [provider] in await provider.unload() }
    }

    /// English only: Malayalam shows the vowel-sign strip, not word suggestions.
    private func requestSuggestions() {
        guard input.language == .english else { return }
        let context = String((textDocumentProxy.documentContextBeforeInput ?? "").suffix(64))
        guard context != lastRequestedContext else { return }
        lastRequestedContext = context
        suggestions.request(contextBefore: context)
    }
}
