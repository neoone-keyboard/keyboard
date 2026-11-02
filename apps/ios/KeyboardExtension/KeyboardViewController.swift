import UIKit
import os
import KeyboardCore

final class KeyboardViewController: UIInputViewController {
    private let log = Logger(subsystem: "me.jishnu.keyboard.extension", category: "keyboard")
    private let budget = MemoryBudget.keyboardExtension
    private let suggestionBar = SuggestionBarView()
    private var keysView: KeysView!

    private lazy var input = InputController(output: ProxyTextOutput { [unowned self] in
        self.textDocumentProxy
    })

    private let provider = EngineSuggestionProvider()
    private var lastRequestedContext: String?

    private lazy var suggestions = SuggestionCoordinator(provider: provider) {
        [weak self] in self?.suggestionBar.show($0)
    }

    override func viewDidLoad() {
        super.viewDidLoad()
        keysView = KeysView(layout: .qwerty) { [unowned self] button in
            button.addTarget(self, action: #selector(handleInputModeList(from:with:)), for: .allTouchEvents)
        }
        keysView.onAction = { [unowned self] in didTap($0) }
        keysView.refreshLabels(input.label(for:))

        let stack = UIStackView(arrangedSubviews: [suggestionBar, keysView])
        stack.axis = .vertical
        stack.spacing = 6
        stack.translatesAutoresizingMaskIntoConstraints = false
        view.addSubview(stack)

        let height = view.heightAnchor.constraint(equalToConstant: 270)
        height.priority = .defaultHigh + 1
        NSLayoutConstraint.activate([
            height,
            suggestionBar.heightAnchor.constraint(equalToConstant: 40),
            stack.leadingAnchor.constraint(equalTo: view.leadingAnchor, constant: 4),
            stack.trailingAnchor.constraint(equalTo: view.trailingAnchor, constant: -4),
            stack.topAnchor.constraint(equalTo: view.topAnchor, constant: 4),
            stack.bottomAnchor.constraint(equalTo: view.safeAreaLayoutGuide.bottomAnchor, constant: -4),
        ])
        log.info("Keyboard loaded; footprint \(MemoryMonitor.footprintBytes(), privacy: .public) bytes")
    }

    override func viewWillLayoutSubviews() {
        keysView.nextKeyboardButton?.isHidden = !needsInputModeSwitchKey
        super.viewWillLayoutSubviews()
    }

    override func textDidChange(_ textInput: UITextInput?) {
        super.textDidChange(textInput)
        checkMemoryBudget()
        requestSuggestions()
    }

    override func didReceiveMemoryWarning() {
        super.didReceiveMemoryWarning()
        log.warning("Memory warning; prediction off")
        shedPrediction()
    }

    private func didTap(_ action: KeyAction) {
        let shiftBefore = input.shift
        input.handle(action, at: ProcessInfo.processInfo.systemUptime)
        if input.shift != shiftBefore {
            keysView.refreshLabels(input.label(for:))
        }
        checkMemoryBudget()
        requestSuggestions()
    }

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

    private func requestSuggestions() {
        let context = String((textDocumentProxy.documentContextBeforeInput ?? "").suffix(64))
        guard context != lastRequestedContext else { return }
        lastRequestedContext = context
        suggestions.request(contextBefore: context)
    }
}
