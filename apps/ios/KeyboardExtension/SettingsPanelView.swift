import UIKit
import KeyboardCore

/// The keyboard's own settings, shown over the keys after a long-press on the
/// language key. Every change is saved at once and reported through `onChange`
/// so the keyboard re-applies it live.
///
/// Laid out like the app's home screen: a fixed title bar, then rounded section cards with
/// small uppercase headers, scrolling within the keys' height. Languages first, then
/// Suggestions, Typing, Look, and Your words / Forget / diagnostics.
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
    private let languageCard = SettingsCard()
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

    private let scroll = UIScrollView()
    private var cards: [SettingsCard] = []
    private var headers: [UILabel] = []
    private var rowLabels: [UILabel] { [accentLabel, numberRowLabel, clicksLabel, autocorrectLabel, emojiLabel, suggestionsLabel] }

    private static let themes: [ThemeChoice] = [.system, .light, .dark]
    private static let swatchSize: CGFloat = 28
    /// The side margin of the cards, and of everything in them: trailing switches stay
    /// inside it (iOS 26's wider UISwitch reached the panel's edge before).
    static let margin: CGFloat = 16
    private static let rowHeight: CGFloat = 44
    private static let titleHeight: CGFloat = 44

    override init(frame: CGRect) {
        super.init(frame: frame)
        layer.cornerRadius = 8
        clipsToBounds = true

        titleLabel.text = "Keyboard settings"
        titleLabel.font = .systemFont(ofSize: 17, weight: .semibold)
        titleLabel.accessibilityTraits = .header
        doneButton.setTitle("Done", for: .normal)
        doneButton.titleLabel?.font = .systemFont(ofSize: 17, weight: .semibold)
        doneButton.accessibilityIdentifier = "settings-done"
        doneButton.addAction(UIAction { [weak self] _ in self?.onDone?() }, for: .touchUpInside)
        doneButton.setContentHuggingPriority(.required, for: .horizontal)

        themeControl.accessibilityIdentifier = "theme"
        themeControl.accessibilityLabel = "Appearance"
        themeControl.addAction(UIAction { [weak self] _ in self?.themeChanged() }, for: .valueChanged)

        accentLabel.text = "Accent"
        swatches = AccentChoice.allCases.map { choice in
            let button = UIButton(type: .custom)
            button.backgroundColor = KeyboardTheme.accentColor(choice)
            button.layer.cornerRadius = Self.swatchSize / 2
            button.accessibilityLabel = choice.rawValue.capitalized
            button.accessibilityIdentifier = "accent-\(choice.rawValue)"
            button.addAction(UIAction { [weak self] _ in self?.update { $0.accent = choice } }, for: .touchUpInside)
            NSLayoutConstraint.activate([
                button.widthAnchor.constraint(equalToConstant: Self.swatchSize),
                button.heightAnchor.constraint(equalToConstant: Self.swatchSize),
            ])
            return button
        }

        numberRowLabel.text = "Number row"
        numberRowSwitch.accessibilityIdentifier = "number-row"
        numberRowSwitch.addAction(UIAction { [weak self] _ in
            guard let self else { return }
            update { $0.numberRow = self.numberRowSwitch.isOn }
        }, for: .valueChanged)
        clicksLabel.text = "Key clicks"
        clicksSwitch.accessibilityIdentifier = "key-clicks"
        clicksSwitch.addAction(UIAction { [weak self] _ in
            guard let self else { return }
            update { $0.keyClicks = self.clicksSwitch.isOn }
        }, for: .valueChanged)
        autocorrectLabel.text = "Autocorrect"
        autocorrectSwitch.accessibilityIdentifier = "autocorrect"
        autocorrectSwitch.addAction(UIAction { [weak self] _ in
            guard let self else { return }
            update { $0.autocorrect = self.autocorrectSwitch.isOn }
        }, for: .valueChanged)
        emojiLabel.text = "Emoji suggestions"
        emojiSwitch.accessibilityIdentifier = "emoji-suggestions"
        emojiSwitch.addAction(UIAction { [weak self] _ in
            guard let self else { return }
            update { $0.emojiSuggestions = self.emojiSwitch.isOn }
        }, for: .valueChanged)
        // The labels say what each switch is to VoiceOver.
        numberRowSwitch.accessibilityLabel = numberRowLabel.text
        clicksSwitch.accessibilityLabel = clicksLabel.text
        autocorrectSwitch.accessibilityLabel = autocorrectLabel.text
        emojiSwitch.accessibilityLabel = emojiLabel.text

        suggestionsLabel.text = "Words in the bar"
        suggestionsControl.accessibilityIdentifier = "suggestion-count"
        suggestionsControl.accessibilityLabel = "Suggestions"
        suggestionsControl.addAction(UIAction { [weak self] _ in self?.suggestionCountChanged() }, for: .valueChanged)

        languagesHint.text = "Add languages in the NeoOne Keyboard app."
        languagesHint.font = .systemFont(ofSize: 13)
        languagesHint.numberOfLines = 0

        yourWordsButton.setTitle("Your words", for: .normal)
        yourWordsButton.titleLabel?.font = .systemFont(ofSize: 15)
        yourWordsButton.contentHorizontalAlignment = .leading
        yourWordsButton.accessibilityIdentifier = "your-words"
        yourWordsButton.addAction(UIAction { [weak self] _ in self?.onYourWords?() }, for: .touchUpInside)
        forgetButton.titleLabel?.font = .systemFont(ofSize: 15)
        forgetButton.contentHorizontalAlignment = .leading
        forgetButton.accessibilityIdentifier = "forget-learned"
        forgetButton.addAction(UIAction { [weak self] _ in self?.forgetTapped() }, for: .touchUpInside)
        diagnosticsButton.setTitle("Insert diagnostics", for: .normal)
        diagnosticsButton.titleLabel?.font = .systemFont(ofSize: 15)
        diagnosticsButton.contentHorizontalAlignment = .leading
        diagnosticsButton.accessibilityIdentifier = "insert-diagnostics"
        diagnosticsButton.addAction(UIAction { [weak self] _ in self?.onInsertDiagnostics?() }, for: .touchUpInside)

        for label in rowLabels {
            label.font = .systemFont(ofSize: 15)
        }

        // The sections, in order.
        let suggestionsCard = SettingsCard(rows: [Self.row(suggestionsLabel, trailing: suggestionsControl)])
        let typingCard = SettingsCard(rows: [
            Self.row(numberRowLabel, trailing: numberRowSwitch),
            Self.row(autocorrectLabel, trailing: autocorrectSwitch),
            Self.row(emojiLabel, trailing: emojiSwitch),
            Self.row(clicksLabel, trailing: clicksSwitch),
        ])
        let swatchStack = UIStackView(arrangedSubviews: swatches)
        swatchStack.spacing = 14
        let themeRow = Self.padded(themeControl, vertical: 8)
        let lookCard = SettingsCard(rows: [themeRow, Self.row(accentLabel, trailing: swatchStack)])
        let wordsCard = SettingsCard(rows: [
            Self.row(yourWordsButton, trailing: chevron()),
            Self.row(forgetButton, trailing: nil),
            Self.row(diagnosticsButton, trailing: nil),
        ])
        cards = [languageCard, suggestionsCard, typingCard, lookCard, wordsCard]

        let hint = Self.padded(languagesHint, vertical: 0)
        let stack = UIStackView(arrangedSubviews: [
            header("Languages"), languageCard, hint,
            header("Suggestions"), suggestionsCard,
            header("Typing"), typingCard,
            header("Look"), lookCard,
            header("Learned words"), wordsCard,
        ])
        stack.axis = .vertical
        stack.spacing = 6
        for view in [languageCard, hint, suggestionsCard, typingCard, lookCard] {
            stack.setCustomSpacing(view === languageCard ? 6 : 14, after: view)
        }
        stack.translatesAutoresizingMaskIntoConstraints = false

        // The title bar stays put; the sections scroll under it within the keys' height.
        let titleBar = UIStackView(arrangedSubviews: [titleLabel, doneButton])
        titleBar.axis = .horizontal
        titleBar.alignment = .center
        titleBar.translatesAutoresizingMaskIntoConstraints = false

        scroll.alwaysBounceVertical = false
        // iOS 26 blurs scroll content near each edge, which smears the rows in a short panel.
        for edge in [scroll.topEdgeEffect, scroll.bottomEdgeEffect, scroll.leftEdgeEffect, scroll.rightEdgeEffect] {
            edge.isHidden = true
        }
        scroll.accessibilityIdentifier = "settings-scroll"
        scroll.translatesAutoresizingMaskIntoConstraints = false
        scroll.addSubview(stack)
        addSubview(titleBar)
        addSubview(scroll)
        let content = scroll.contentLayoutGuide
        let margin = Self.margin
        NSLayoutConstraint.activate([
            titleBar.leadingAnchor.constraint(equalTo: leadingAnchor, constant: margin + 4),
            titleBar.trailingAnchor.constraint(equalTo: trailingAnchor, constant: -margin),
            titleBar.topAnchor.constraint(equalTo: topAnchor),
            titleBar.heightAnchor.constraint(equalToConstant: Self.titleHeight),
            scroll.leadingAnchor.constraint(equalTo: leadingAnchor),
            scroll.trailingAnchor.constraint(equalTo: trailingAnchor),
            scroll.topAnchor.constraint(equalTo: titleBar.bottomAnchor),
            scroll.bottomAnchor.constraint(equalTo: bottomAnchor),
            stack.leadingAnchor.constraint(equalTo: content.leadingAnchor, constant: margin),
            stack.trailingAnchor.constraint(equalTo: content.trailingAnchor, constant: -margin),
            stack.topAnchor.constraint(equalTo: content.topAnchor, constant: 2),
            stack.bottomAnchor.constraint(equalTo: content.bottomAnchor, constant: -12),
            stack.widthAnchor.constraint(equalTo: scroll.frameLayoutGuide.widthAnchor, constant: -2 * margin),
            themeControl.heightAnchor.constraint(equalToConstant: 32),
            suggestionsControl.widthAnchor.constraint(equalToConstant: 150),
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
        scroll.contentOffset = .zero
    }

    func apply(_ theme: KeyboardTheme) {
        self.theme = theme
        overrideUserInterfaceStyle = theme.isDark ? .dark : .light
        backgroundColor = theme.background
        let card = Self.cardColor(theme)
        for each in cards {
            each.backgroundColor = card
            each.separatorColor = theme.isDark ? UIColor(white: 1, alpha: 0.12) : UIColor(white: 0, alpha: 0.1)
        }
        for label in [titleLabel] + rowLabels + languageSwitches.map(\.label) {
            label.textColor = theme.keyText
        }
        for label in headers + [languagesHint] {
            label.textColor = theme.hintText
        }
        doneButton.tintColor = theme.keyText
        yourWordsButton.tintColor = theme.keyText
        diagnosticsButton.tintColor = theme.keyText
        forgetButton.tintColor = .systemRed
        chevronView?.tintColor = theme.hintText
        for control in [themeControl, suggestionsControl] {
            control.backgroundColor = theme.isDark ? UIColor(white: 0.08, alpha: 1) : UIColor(white: 0, alpha: 0.06)
            control.selectedSegmentTintColor = theme.isDark ? UIColor(white: 0.36, alpha: 1) : .white
            control.setTitleTextAttributes([.foregroundColor: theme.keyText], for: .normal)
            control.setTitleTextAttributes([.foregroundColor: theme.keyText,
                                            .font: UIFont.systemFont(ofSize: 13, weight: .semibold)], for: .selected)
        }
        for toggle in [numberRowSwitch, clicksSwitch, autocorrectSwitch, emojiSwitch] + languageSwitches.map(\.toggle) {
            toggle.onTintColor = theme.accent
        }
        refreshSwatches()
    }

    /// Cards are the key colour on the light keyboard grey (white on grey, as the app's cards
    /// on its grouped grey); on dark, a raised grey a step under the keys.
    private static func cardColor(_ theme: KeyboardTheme) -> UIColor {
        theme.isDark ? UIColor(white: 0.17, alpha: 1) : theme.key
    }

    /// A row per language, styled like the other switch rows. Rebuilt on each opening: the
    /// app may have installed or removed a pack meanwhile.
    private func showLanguages(_ available: [(id: String, name: String)], active: ActiveLanguages) {
        languages = active
        languageOrder = available.map(\.id)
        languageSwitches = available.map { language in
            let label = UILabel()
            label.text = language.name
            label.font = .systemFont(ofSize: 15)
            let toggle = UISwitch()
            toggle.accessibilityLabel = language.name
            toggle.accessibilityIdentifier = "language-\(language.id)"
            toggle.addAction(UIAction { [weak self] _ in self?.languageToggled(language.id) }, for: .valueChanged)
            return (language.id, label, toggle)
        }
        languageCard.setRows(languageSwitches.map { Self.row($0.label, trailing: $0.toggle) })
        if let theme { apply(theme) }
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

    // MARK: Layout pieces

    private var chevronView: UIImageView?

    private func chevron() -> UIView {
        let configuration = UIImage.SymbolConfiguration(pointSize: 13, weight: .semibold)
        let view = UIImageView(image: UIImage(systemName: "chevron.right", withConfiguration: configuration))
        view.contentMode = .center
        view.isAccessibilityElement = false
        chevronView = view
        return view
    }

    /// "LANGUAGES": 12 pt semibold, uppercase, spaced, inset like the app's headers.
    private func header(_ title: String) -> UIView {
        let label = UILabel()
        label.attributedText = NSAttributedString(string: title.uppercased(), attributes: [
            .font: UIFont.systemFont(ofSize: 12, weight: .semibold),
            .kern: 0.6,
        ])
        label.accessibilityLabel = title
        label.accessibilityTraits = .header
        headers.append(label)
        let wrapper = Self.padded(label, vertical: 0)
        wrapper.heightAnchor.constraint(equalToConstant: 18).isActive = true
        return wrapper
    }

    /// A 44 pt row: `leading` at the card's margin, `trailing` (a switch, a control, a
    /// chevron) inside the card's margin on the other side.
    private static func row(_ leading: UIView, trailing: UIView?) -> UIView {
        let row = UIView()
        leading.translatesAutoresizingMaskIntoConstraints = false
        row.addSubview(leading)
        var constraints = [
            row.heightAnchor.constraint(greaterThanOrEqualToConstant: rowHeight),
            leading.leadingAnchor.constraint(equalTo: row.leadingAnchor, constant: margin),
            leading.centerYAnchor.constraint(equalTo: row.centerYAnchor),
        ]
        if let trailing {
            trailing.translatesAutoresizingMaskIntoConstraints = false
            trailing.setContentCompressionResistancePriority(.required, for: .horizontal)
            trailing.setContentHuggingPriority(.required, for: .horizontal)
            row.addSubview(trailing)
            constraints += [
                // iOS 26's switch draws about 2 pt past its frame on the trailing side.
                trailing.trailingAnchor.constraint(equalTo: row.trailingAnchor,
                                                   constant: -(margin + (trailing is UISwitch ? 2 : 0))),
                trailing.centerYAnchor.constraint(equalTo: row.centerYAnchor),
                leading is UIButton
                    ? leading.trailingAnchor.constraint(equalTo: trailing.leadingAnchor, constant: -12)
                    : leading.trailingAnchor.constraint(lessThanOrEqualTo: trailing.leadingAnchor, constant: -12),
            ]
        } else {
            constraints.append(leading.trailingAnchor.constraint(equalTo: row.trailingAnchor, constant: -margin))
        }
        // A button row takes the whole row's height to be easy to hit.
        if leading is UIButton {
            constraints += [leading.topAnchor.constraint(equalTo: row.topAnchor),
                            leading.bottomAnchor.constraint(equalTo: row.bottomAnchor)]
            if trailing == nil { constraints.append(leading.heightAnchor.constraint(equalToConstant: rowHeight)) }
        }
        NSLayoutConstraint.activate(constraints)
        return row
    }

    /// `view` inset by the card margin at the sides (4 pt more for header text, as in the app).
    private static func padded(_ view: UIView, vertical: CGFloat) -> UIView {
        let wrapper = UIView()
        view.translatesAutoresizingMaskIntoConstraints = false
        wrapper.addSubview(view)
        let side: CGFloat = view is UILabel ? 4 : margin
        NSLayoutConstraint.activate([
            view.leadingAnchor.constraint(equalTo: wrapper.leadingAnchor, constant: side),
            view.trailingAnchor.constraint(equalTo: wrapper.trailingAnchor, constant: -side),
            view.topAnchor.constraint(equalTo: wrapper.topAnchor, constant: vertical),
            view.bottomAnchor.constraint(equalTo: wrapper.bottomAnchor, constant: -vertical),
        ])
        return wrapper
    }
}

/// A rounded card of rows with hairlines between them, inset from the leading edge.
private final class SettingsCard: UIView {
    private let stack = UIStackView()
    private var separators: [UIView] = []
    var separatorColor: UIColor = .separator {
        didSet { for separator in separators { separator.backgroundColor = separatorColor } }
    }

    init(rows: [UIView] = []) {
        super.init(frame: .zero)
        layer.cornerRadius = 12
        layer.cornerCurve = .continuous
        clipsToBounds = true
        stack.axis = .vertical
        stack.translatesAutoresizingMaskIntoConstraints = false
        addSubview(stack)
        NSLayoutConstraint.activate([
            stack.leadingAnchor.constraint(equalTo: leadingAnchor),
            stack.trailingAnchor.constraint(equalTo: trailingAnchor),
            stack.topAnchor.constraint(equalTo: topAnchor),
            stack.bottomAnchor.constraint(equalTo: bottomAnchor),
        ])
        setRows(rows)
    }

    required init?(coder: NSCoder) { fatalError("init(coder:) is not used") }

    func setRows(_ rows: [UIView]) {
        for view in stack.arrangedSubviews { view.removeFromSuperview() }
        separators = []
        for (index, row) in rows.enumerated() {
            stack.addArrangedSubview(row)
            guard index < rows.count - 1 else { continue }
            let separator = UIView()
            separator.backgroundColor = separatorColor
            separator.translatesAutoresizingMaskIntoConstraints = false
            row.addSubview(separator)
            NSLayoutConstraint.activate([
                separator.leadingAnchor.constraint(equalTo: row.leadingAnchor, constant: SettingsPanelView.margin),
                separator.trailingAnchor.constraint(equalTo: row.trailingAnchor),
                separator.bottomAnchor.constraint(equalTo: row.bottomAnchor),
                separator.heightAnchor.constraint(equalToConstant: 1 / max(UITraitCollection.current.displayScale, 1)),
            ])
            separators.append(separator)
        }
    }
}
