import UIKit
import KeyboardCore

/// The keyboard's own settings, shown over the keys after a long-press on the
/// language key. Every change is saved at once and reported through `onChange`
/// so the keyboard re-applies it live.
final class SettingsPanelView: UIView {
    var onChange: (@MainActor (KeyboardSettings) -> Void)?
    var onDone: (@MainActor () -> Void)?
    var onForgetLearned: (@MainActor () -> Void)?
    /// Opens "Your words" (`YourWordsView`).
    var onYourWords: (@MainActor () -> Void)?
    /// A language switched on or off; the new choice, already within the rules.
    var onLanguagesChange: (@MainActor (ActiveLanguages) -> Void)?
    var onInsertDiagnostics: (@MainActor () -> Void)?

    private(set) var settings = KeyboardSettings()
    private var theme: KeyboardTheme?

    private let titleLabel = UILabel()
    private let doneButton = UIButton(type: .system)
    private let themeControl = UISegmentedControl(items: ["Match system", "Light", "Dark"])
    private let accentLabel = UILabel()
    private var swatches: [UIButton] = []
    private let numberRowLabel = UILabel()
    private let numberRowSwitch = UISwitch()
    private let clicksLabel = UILabel()
    private let clicksSwitch = UISwitch()
    private let autocorrectLabel = UILabel()
    private let autocorrectSwitch = UISwitch()
    private let emojiLabel = UILabel()
    private let emojiSwitch = UISwitch()
    /// How many suggestions the bar holds (`KeyboardSettings.suggestionCounts`).
    private let suggestionsLabel = UILabel()
    private let suggestionsControl = UISegmentedControl(items: KeyboardSettings.suggestionCounts.map(String.init))
    private let yourWordsButton = UIButton(type: .system)
    /// One switch per language the keyboard has (English, then its packs), built in `show`.
    private let languagesLabel = UILabel()
    private let languageRows = UIStackView()
    private let languagesHint = UILabel()
    private var languageSwitches: [(id: String, label: UILabel, toggle: UISwitch)] = []
    private var languageOrder: [String] = []
    private(set) var languages = ActiveLanguages([ActiveLanguages.english])
    private let forgetButton = UIButton(type: .system)
    private let diagnosticsButton = UIButton(type: .system)
    /// Forgetting needs a second tap, so a stray touch can't wipe learned words.
    private var forgetArmed = false
    /// Puts the Forget row back to its first state a few seconds after a tap.
    private var forgetRevert: Task<Void, Never>?

    private static let themes: [ThemeChoice] = [.system, .light, .dark]
    private static let swatchSize: CGFloat = 28

    override init(frame: CGRect) {
        super.init(frame: frame)
        layer.cornerRadius = 8

        titleLabel.text = "Keyboard settings"
        titleLabel.font = .systemFont(ofSize: 17, weight: .semibold)
        doneButton.setTitle("Done", for: .normal)
        doneButton.titleLabel?.font = .systemFont(ofSize: 17, weight: .semibold)
        doneButton.addAction(UIAction { [weak self] _ in self?.onDone?() }, for: .touchUpInside)
        doneButton.setContentHuggingPriority(.required, for: .horizontal)
        let header = UIStackView(arrangedSubviews: [titleLabel, doneButton])

        themeControl.addAction(UIAction { [weak self] _ in self?.themeChanged() }, for: .valueChanged)

        accentLabel.text = "Accent"
        swatches = AccentChoice.allCases.map { choice in
            let button = UIButton(type: .custom)
            button.backgroundColor = KeyboardTheme.accentColor(choice)
            button.layer.cornerRadius = Self.swatchSize / 2
            button.accessibilityLabel = choice.rawValue.capitalized
            button.addAction(UIAction { [weak self] _ in self?.update { $0.accent = choice } }, for: .touchUpInside)
            NSLayoutConstraint.activate([
                button.widthAnchor.constraint(equalToConstant: Self.swatchSize),
                button.heightAnchor.constraint(equalToConstant: Self.swatchSize),
            ])
            return button
        }
        let swatchStack = UIStackView(arrangedSubviews: swatches)
        swatchStack.spacing = 14
        let accentRow = UIStackView(arrangedSubviews: [accentLabel, UIView(), swatchStack])

        numberRowLabel.text = "Number row"
        numberRowSwitch.addAction(UIAction { [weak self] _ in
            guard let self else { return }
            update { $0.numberRow = self.numberRowSwitch.isOn }
        }, for: .valueChanged)
        clicksLabel.text = "Key clicks"
        clicksSwitch.addAction(UIAction { [weak self] _ in
            guard let self else { return }
            update { $0.keyClicks = self.clicksSwitch.isOn }
        }, for: .valueChanged)
        autocorrectLabel.text = "Autocorrect"
        autocorrectSwitch.addAction(UIAction { [weak self] _ in
            guard let self else { return }
            update { $0.autocorrect = self.autocorrectSwitch.isOn }
        }, for: .valueChanged)
        emojiLabel.text = "Emoji suggestions"
        emojiSwitch.addAction(UIAction { [weak self] _ in
            guard let self else { return }
            update { $0.emojiSuggestions = self.emojiSwitch.isOn }
        }, for: .valueChanged)
        suggestionsLabel.text = "Suggestions"
        suggestionsControl.accessibilityIdentifier = "suggestion-count"
        suggestionsControl.addAction(UIAction { [weak self] _ in self?.suggestionCountChanged() }, for: .valueChanged)
        languagesLabel.text = "Languages"
        languagesLabel.font = .systemFont(ofSize: 15, weight: .semibold)
        languageRows.axis = .vertical
        languageRows.spacing = 6
        languagesHint.text = "Add languages in the NeoOne Keyboard app."
        languagesHint.font = .systemFont(ofSize: 13)
        languagesHint.numberOfLines = 0
        yourWordsButton.setTitle("Your words", for: .normal)
        yourWordsButton.setImage(UIImage(systemName: "chevron.right"), for: .normal)
        yourWordsButton.titleLabel?.font = .systemFont(ofSize: 15)
        // The chevron after the title, and both at the left like the other rows.
        yourWordsButton.semanticContentAttribute = .forceRightToLeft
        yourWordsButton.contentHorizontalAlignment = .left
        yourWordsButton.addAction(UIAction { [weak self] _ in self?.onYourWords?() }, for: .touchUpInside)
        forgetButton.titleLabel?.font = .systemFont(ofSize: 15)
        forgetButton.contentHorizontalAlignment = .leading
        forgetButton.addAction(UIAction { [weak self] _ in self?.forgetTapped() }, for: .touchUpInside)
        diagnosticsButton.setTitle("Insert diagnostics", for: .normal)
        diagnosticsButton.titleLabel?.font = .systemFont(ofSize: 15)
        diagnosticsButton.contentHorizontalAlignment = .leading
        diagnosticsButton.addAction(UIAction { [weak self] _ in self?.onInsertDiagnostics?() }, for: .touchUpInside)
        let numberRow = UIStackView(arrangedSubviews: [numberRowLabel, UIView(), numberRowSwitch])
        let clicksRow = UIStackView(arrangedSubviews: [clicksLabel, UIView(), clicksSwitch])
        let autocorrectRow = UIStackView(arrangedSubviews: [autocorrectLabel, UIView(), autocorrectSwitch])
        let emojiRow = UIStackView(arrangedSubviews: [emojiLabel, UIView(), emojiSwitch])
        let suggestionsRow = UIStackView(arrangedSubviews: [suggestionsLabel, UIView(), suggestionsControl])

        for row in [header, accentRow, numberRow, clicksRow, autocorrectRow, emojiRow, suggestionsRow] {
            row.axis = .horizontal
            row.alignment = .center
        }
        for label in [accentLabel, numberRowLabel, clicksLabel, autocorrectLabel, emojiLabel, suggestionsLabel] {
            label.font = .systemFont(ofSize: 15)
        }

        let stack = UIStackView(arrangedSubviews: [
            header, themeControl, accentRow, numberRow, clicksRow, autocorrectRow, emojiRow, suggestionsRow, languagesLabel, languageRows, languagesHint,
            yourWordsButton,
            forgetButton, diagnosticsButton,
        ])
        stack.axis = .vertical
        stack.spacing = 6
        stack.translatesAutoresizingMaskIntoConstraints = false

        // Scrolls only if the keys area is shorter than the panel's content.
        let scroll = UIScrollView()
        scroll.alwaysBounceVertical = false
        // iOS 26 blurs scroll content near each edge, which smears the rows in a short panel.
        for edge in [scroll.topEdgeEffect, scroll.bottomEdgeEffect, scroll.leftEdgeEffect, scroll.rightEdgeEffect] {
            edge.isHidden = true
        }
        scroll.translatesAutoresizingMaskIntoConstraints = false
        scroll.addSubview(stack)
        addSubview(scroll)
        let content = scroll.contentLayoutGuide
        NSLayoutConstraint.activate([
            scroll.leadingAnchor.constraint(equalTo: leadingAnchor),
            scroll.trailingAnchor.constraint(equalTo: trailingAnchor),
            scroll.topAnchor.constraint(equalTo: topAnchor),
            scroll.bottomAnchor.constraint(equalTo: bottomAnchor),
            stack.leadingAnchor.constraint(equalTo: content.leadingAnchor, constant: 12),
            stack.trailingAnchor.constraint(equalTo: content.trailingAnchor, constant: -12),
            stack.topAnchor.constraint(equalTo: content.topAnchor, constant: 4),
            stack.bottomAnchor.constraint(equalTo: content.bottomAnchor, constant: -8),
            stack.widthAnchor.constraint(equalTo: scroll.frameLayoutGuide.widthAnchor, constant: -24),
            header.heightAnchor.constraint(equalToConstant: 36),
            themeControl.heightAnchor.constraint(equalToConstant: 30),
            accentRow.heightAnchor.constraint(equalToConstant: 36),
            suggestionsRow.heightAnchor.constraint(equalToConstant: 36),
            suggestionsControl.heightAnchor.constraint(equalToConstant: 30),
            suggestionsControl.widthAnchor.constraint(equalToConstant: 150),
            languagesLabel.heightAnchor.constraint(equalToConstant: 30),
            yourWordsButton.heightAnchor.constraint(equalToConstant: 36),
            forgetButton.heightAnchor.constraint(equalToConstant: 36),
            diagnosticsButton.heightAnchor.constraint(equalToConstant: 36),
        ])
    }

    required init?(coder: NSCoder) { fatalError("init(coder:) is not used") }

    /// Shows `settings` and the language switches (`available`: id and name, English first,
    /// `active` on) in the controls without reporting a change.
    func show(_ settings: KeyboardSettings, available: [(id: String, name: String)], active: ActiveLanguages) {
        self.settings = settings
        showLanguages(available, active: active)
        themeControl.selectedSegmentIndex = Self.themes.firstIndex(of: settings.theme) ?? 0
        numberRowSwitch.isOn = settings.numberRow
        clicksSwitch.isOn = settings.keyClicks
        autocorrectSwitch.isOn = settings.autocorrect
        emojiSwitch.isOn = settings.emojiSuggestions
        suggestionsControl.selectedSegmentIndex = KeyboardSettings.suggestionCounts.firstIndex(of: settings.suggestionCount) ?? 1
        setForgetArmed(false)
        refreshSwatches()
    }

    func apply(_ theme: KeyboardTheme) {
        self.theme = theme
        overrideUserInterfaceStyle = theme.isDark ? .dark : .light
        backgroundColor = theme.background
        for label in [titleLabel, accentLabel, numberRowLabel, clicksLabel, autocorrectLabel, emojiLabel, suggestionsLabel, languagesLabel]
            + languageSwitches.map(\.label) {
            label.textColor = theme.keyText
        }
        languagesHint.textColor = theme.hintText
        doneButton.tintColor = theme.keyText
        yourWordsButton.tintColor = theme.keyText
        forgetButton.tintColor = theme.keyText
        diagnosticsButton.tintColor = theme.keyText
        for control in [themeControl, suggestionsControl] {
            control.backgroundColor = theme.specialKey
            control.selectedSegmentTintColor = theme.key
            control.setTitleTextAttributes([.foregroundColor: theme.keyText], for: .normal)
            control.setTitleTextAttributes([.foregroundColor: theme.keyText], for: .selected)
        }
        for toggle in [numberRowSwitch, clicksSwitch, autocorrectSwitch, emojiSwitch] + languageSwitches.map(\.toggle) {
            toggle.onTintColor = theme.accent
        }
        refreshSwatches()
    }

    /// A row per language, styled like the other switch rows. Rebuilt on each opening: the
    /// app may have installed or removed a pack meanwhile.
    private func showLanguages(_ available: [(id: String, name: String)], active: ActiveLanguages) {
        languages = active
        languageOrder = available.map(\.id)
        for row in languageRows.arrangedSubviews { row.removeFromSuperview() }
        languageSwitches = available.map { language in
            let label = UILabel()
            label.text = language.name
            label.font = .systemFont(ofSize: 15)
            let toggle = UISwitch()
            toggle.accessibilityLabel = language.name
            toggle.accessibilityIdentifier = "language-\(language.id)"
            toggle.addAction(UIAction { [weak self] _ in self?.languageToggled(language.id) }, for: .valueChanged)
            let row = UIStackView(arrangedSubviews: [label, UIView(), toggle])
            row.axis = .horizontal
            row.alignment = .center
            languageRows.addArrangedSubview(row)
            return (language.id, label, toggle)
        }
        refreshLanguageSwitches()
    }

    private func languageToggled(_ id: String) {
        let next = languages.toggling(id, order: languageOrder)
        guard next != languages else {
            refreshLanguageSwitches()
            return
        }
        languages = next
        refreshLanguageSwitches()
        onLanguagesChange?(next)
    }

    /// At most `ActiveLanguages.maxActive` on: when full the others are disabled; the last one
    /// on can't be turned off.
    private func refreshLanguageSwitches() {
        for (id, _, toggle) in languageSwitches {
            toggle.isOn = languages.isOn(id)
            toggle.isEnabled = languages.canToggle(id)
        }
    }

    private func forgetTapped() {
        if forgetArmed {
            onForgetLearned?()
            setForgetArmed(false)
            forgetButton.setTitle("Learned words forgotten", for: .normal)
            scheduleForgetRevert(resetting: false)
        } else {
            setForgetArmed(true)
        }
    }

    private func setForgetArmed(_ armed: Bool) {
        forgetArmed = armed
        forgetButton.setTitle(armed ? "Tap again to forget all learned words" : "Forget learned words", for: .normal)
        scheduleForgetRevert(resetting: armed)
    }

    /// After a few seconds the armed (or confirmed) row goes back to "Forget learned words".
    private func scheduleForgetRevert(resetting armed: Bool) {
        forgetRevert?.cancel()
        forgetRevert = nil
        guard armed || !forgetArmed && forgetButton.title(for: .normal) == "Learned words forgotten" else { return }
        forgetRevert = Task { @MainActor [weak self] in
            try? await Task.sleep(for: .seconds(3))
            guard !Task.isCancelled else { return }
            self?.setForgetArmed(false)
        }
    }

    private func themeChanged() {
        let index = themeControl.selectedSegmentIndex
        guard Self.themes.indices.contains(index) else { return }
        update { $0.theme = Self.themes[index] }
    }

    private func suggestionCountChanged() {
        let index = suggestionsControl.selectedSegmentIndex
        guard KeyboardSettings.suggestionCounts.indices.contains(index) else { return }
        update { $0.suggestionCount = KeyboardSettings.suggestionCounts[index] }
    }

    private func update(_ change: (inout KeyboardSettings) -> Void) {
        change(&settings)
        settings.save(to: .standard)
        refreshSwatches()
        onChange?(settings)
    }

    /// Rings the selected accent swatch.
    private func refreshSwatches() {
        for (button, choice) in zip(swatches, AccentChoice.allCases) {
            let selected = choice == settings.accent
            button.layer.borderWidth = selected ? 3 : 0
            button.layer.borderColor = (theme?.keyText ?? .label).cgColor
            button.accessibilityTraits = selected ? [.button, .selected] : .button
        }
    }
}
