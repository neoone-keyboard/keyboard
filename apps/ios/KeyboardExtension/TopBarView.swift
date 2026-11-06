import UIKit
import KeyboardCore

/// The strip above the keys: three suggestion slots, or after a Malayalam
/// consonant the best completion on the left and the vowel signs scrolling
/// sideways at full size beside it. Holding a suggestion slot asks whether to
/// remove its word (`SuggestionRemoval`).
final class TopBarView: UIView {
    var onVowelSign: (@MainActor (String) -> Void)?
    var onSuggestion: (@MainActor (SuggestionSlot) -> Void)?
    /// The user confirmed removing a word held in the bar.
    var onRemoveSuggestion: (@MainActor (String) -> Void)?

    private let suggestionStack = UIStackView()
    private let vowelRow = UIStackView()
    private let completionButton = UIButton(type: .custom)
    private var completion: Suggestion?
    private let vowelScroll = UIScrollView()
    private let vowelStack = UIStackView()
    private var theme: KeyboardTheme?
    /// What is showing now, so unchanged content isn't rebuilt on every keystroke.
    private var shownContent: [String] = []
    /// The consonant the vowel strip is drawn on; nil while suggestions show.
    private var stripBase: String?
    /// "Remove “word”?" with Remove and Cancel, over the slots while `removal` is set.
    private let confirmRow = UIStackView()
    private let confirmLabel = UILabel()
    private let removeButton = UIButton(type: .custom)
    private let cancelButton = UIButton(type: .custom)
    private var removal: SuggestionRemoval?
    private var removalTimeout: Task<Void, Never>?

    /// Whether the vowel strip is showing (rather than the suggestion slots).
    var showsVowelSigns: Bool { stripBase != nil }

    /// Width of one vowel-sign button. Wide enough to tap without zooming;
    /// the last one is cut off at the edge, which shows the row scrolls.
    private static let vowelButtonWidth: CGFloat = 52

    override init(frame: CGRect) {
        super.init(frame: frame)
        suggestionStack.axis = .horizontal
        suggestionStack.distribution = .fillEqually
        suggestionStack.spacing = 3

        vowelStack.axis = .horizontal
        vowelStack.spacing = 4
        vowelScroll.showsHorizontalScrollIndicator = false
        vowelScroll.alwaysBounceHorizontal = true
        // iOS 26 blurs scroll content near each edge; in a 40 pt strip that blurs every button.
        for edge in [vowelScroll.topEdgeEffect, vowelScroll.bottomEdgeEffect,
                     vowelScroll.leftEdgeEffect, vowelScroll.rightEdgeEffect] {
            edge.isHidden = true
        }
        vowelScroll.addSubview(vowelStack)

        completionButton.titleLabel?.font = .systemFont(ofSize: 17)
        completionButton.titleLabel?.adjustsFontSizeToFitWidth = true
        completionButton.titleLabel?.minimumScaleFactor = 0.6
        completionButton.setContentHuggingPriority(.required, for: .horizontal)
        completionButton.addAction(UIAction { [weak self] _ in
            guard let self, let completion = self.completion else { return }
            self.onSuggestion?(SuggestionSlot(suggestion: completion, quoted: false))
        }, for: .touchUpInside)
        vowelRow.axis = .horizontal
        vowelRow.spacing = 4
        vowelRow.addArrangedSubview(completionButton)
        vowelRow.addArrangedSubview(vowelScroll)

        confirmLabel.font = .systemFont(ofSize: 16)
        confirmLabel.adjustsFontSizeToFitWidth = true
        confirmLabel.minimumScaleFactor = 0.6
        confirmLabel.lineBreakMode = .byTruncatingMiddle
        // A long word shrinks or truncates in the label; the buttons keep their size.
        confirmLabel.setContentCompressionResistancePriority(.defaultLow, for: .horizontal)
        for (button, title) in [(removeButton, "Remove"), (cancelButton, "Cancel")] {
            button.setTitle(title, for: .normal)
            button.titleLabel?.font = .systemFont(ofSize: 16, weight: button === removeButton ? .semibold : .regular)
            button.layer.cornerRadius = 6
            button.setContentCompressionResistancePriority(.required, for: .horizontal)
            if let title = button.titleLabel {
                button.widthAnchor.constraint(equalTo: title.widthAnchor, constant: 24).isActive = true
            }
        }
        removeButton.addAction(UIAction { [weak self] _ in self?.confirmRemoval() }, for: .touchUpInside)
        cancelButton.addAction(UIAction { [weak self] _ in self?.cancelRemoval() }, for: .touchUpInside)
        confirmRow.axis = .horizontal
        confirmRow.spacing = 6
        confirmRow.addArrangedSubview(confirmLabel)
        confirmRow.addArrangedSubview(cancelButton)
        confirmRow.addArrangedSubview(removeButton)
        confirmRow.isLayoutMarginsRelativeArrangement = true
        confirmRow.directionalLayoutMargins = NSDirectionalEdgeInsets(top: 4, leading: 8, bottom: 4, trailing: 4)
        confirmRow.isHidden = true

        for view in [suggestionStack, vowelRow, vowelStack, confirmRow] {
            view.translatesAutoresizingMaskIntoConstraints = false
        }
        addSubview(suggestionStack)
        addSubview(vowelRow)
        addSubview(confirmRow)
        let content = vowelScroll.contentLayoutGuide
        NSLayoutConstraint.activate([
            suggestionStack.leadingAnchor.constraint(equalTo: leadingAnchor),
            suggestionStack.trailingAnchor.constraint(equalTo: trailingAnchor),
            suggestionStack.topAnchor.constraint(equalTo: topAnchor),
            suggestionStack.bottomAnchor.constraint(equalTo: bottomAnchor),
            vowelRow.leadingAnchor.constraint(equalTo: leadingAnchor),
            vowelRow.trailingAnchor.constraint(equalTo: trailingAnchor),
            vowelRow.topAnchor.constraint(equalTo: topAnchor),
            vowelRow.bottomAnchor.constraint(equalTo: bottomAnchor),
            confirmRow.leadingAnchor.constraint(equalTo: leadingAnchor),
            confirmRow.trailingAnchor.constraint(equalTo: trailingAnchor),
            confirmRow.topAnchor.constraint(equalTo: topAnchor),
            confirmRow.bottomAnchor.constraint(equalTo: bottomAnchor),
            completionButton.widthAnchor.constraint(lessThanOrEqualTo: widthAnchor, multiplier: 0.4),
            vowelStack.leadingAnchor.constraint(equalTo: content.leadingAnchor),
            vowelStack.trailingAnchor.constraint(equalTo: content.trailingAnchor),
            vowelStack.topAnchor.constraint(equalTo: content.topAnchor),
            vowelStack.bottomAnchor.constraint(equalTo: content.bottomAnchor),
            vowelStack.heightAnchor.constraint(equalTo: vowelScroll.frameLayoutGuide.heightAnchor),
        ])
        vowelRow.isHidden = true
    }

    required init?(coder: NSCoder) { fatalError("init(coder:) is not used") }

    func apply(_ theme: KeyboardTheme) {
        self.theme = theme
        shownContent = []
        confirmLabel.textColor = theme.keyText
        removeButton.backgroundColor = theme.accent
        removeButton.setTitleColor(theme.onAccent, for: .normal)
        removeButton.setTitleColor(theme.hintText, for: .highlighted)
        cancelButton.backgroundColor = theme.specialKey
        cancelButton.setTitleColor(theme.keyText, for: .normal)
        cancelButton.setTitleColor(theme.hintText, for: .highlighted)
    }

    /// Drops the "Remove …?" question, if showing: on Cancel, its timeout, or any typing.
    func cancelRemoval() {
        removalTimeout?.cancel()
        removalTimeout = nil
        guard removal != nil else { return }
        removal = nil
        updateVisibility()
    }

    @objc private func slotHeld(_ hold: SlotHold) {
        guard hold.state == .began, let slot = hold.slot else { return }
        askToRemove(slot)
    }

    private func askToRemove(_ slot: SuggestionSlot) {
        guard let asked = SuggestionRemoval(slot: slot) else { return }
        removal = asked
        confirmLabel.text = asked.prompt
        updateVisibility()
        removalTimeout?.cancel()
        removalTimeout = Task { @MainActor [weak self] in
            try? await Task.sleep(for: SuggestionRemoval.timeout)
            guard !Task.isCancelled else { return }
            self?.cancelRemoval()
        }
    }

    private func confirmRemoval() {
        guard let word = removal?.word else { return }
        cancelRemoval()
        onRemoveSuggestion?(word)
    }

    /// The question covers whichever row is showing; the row underneath keeps updating.
    private func updateVisibility() {
        let asking = removal != nil
        confirmRow.isHidden = !asking
        suggestionStack.isHidden = asking || stripBase != nil
        vowelRow.isHidden = asking || stripBase == nil
    }

    /// Left, middle, right. The middle slot is what space commits, so it is bold.
    func showSuggestions(_ bar: SuggestionBar) {
        let content = ["suggestions"] + bar.slots.map { slot in slot.map { "\($0.title)|\($0.quoted)" } ?? "" }
        guard content != shownContent else { return }
        shownContent = content
        stripBase = nil
        updateVisibility()
        replaceItems(in: suggestionStack, with: bar.slots.enumerated().map { index, slot in
            let button = UIButton(type: .custom)
            guard let slot else { return button }
            button.setTitle(slot.title, for: .normal)
            button.titleLabel?.font = .systemFont(ofSize: 17, weight: index == 1 ? .semibold : .regular)
            button.titleLabel?.adjustsFontSizeToFitWidth = true
            button.titleLabel?.minimumScaleFactor = 0.6
            button.setTitleColor(theme?.keyText, for: .normal)
            button.setTitleColor(theme?.hintText, for: .highlighted)
            button.addAction(UIAction { [weak self] _ in self?.onSuggestion?(slot) }, for: .touchUpInside)
            // An emoji is drawn larger, and has no word to remove.
            if slot.suggestion.kind == .emoji {
                button.titleLabel?.font = .systemFont(ofSize: 24)
                return button
            }
            // Once the hold is recognised the button's touch is cancelled, so it doesn't commit.
            let hold = SlotHold(target: self, action: #selector(slotHeld(_:)))
            hold.slot = slot
            hold.minimumPressDuration = SuggestionRemoval.holdDuration
            button.addGestureRecognizer(hold)
            button.accessibilityCustomActions = [UIAccessibilityCustomAction(name: "Remove suggestion") { [weak self] _ in
                self?.askToRemove(slot)
                return true
            }]
            return button
        })
    }

    /// A short note in the middle of the bar, in place of the slots ("Swipe again").
    func showMessage(_ text: String) {
        let content = ["message", text]
        guard content != shownContent else { return }
        shownContent = content
        stripBase = nil
        updateVisibility()
        let label = UILabel()
        label.text = text
        label.textAlignment = .center
        label.font = .systemFont(ofSize: 16)
        label.textColor = theme?.hintText
        replaceItems(in: suggestionStack, with: [UIView(), label, UIView()])
    }

    /// The best completion (if any) on the left, then one button per vowel
    /// sign drawn on `base` (കാ കി കീ …). Tapping a sign inserts the sign alone.
    func showVowelSigns(base: String, signs: [String], completion: Suggestion?) {
        let content = ["vowels", base, completion?.text ?? ""] + signs
        guard content != shownContent else { return }
        shownContent = content
        self.completion = completion
        completionButton.isHidden = completion == nil
        completionButton.setTitle(completion?.text, for: .normal)
        completionButton.setTitleColor(theme?.keyText, for: .normal)
        completionButton.setTitleColor(theme?.hintText, for: .highlighted)
        replaceItems(in: vowelStack, with: signs.map { sign in
            let button = UIButton(type: .system)
            button.setTitle(base + sign, for: .normal)
            button.titleLabel?.font = .systemFont(ofSize: 20)
            button.setTitleColor(theme?.onAccent, for: .normal)
            button.backgroundColor = theme?.accent
            button.layer.cornerRadius = 6
            button.widthAnchor.constraint(equalToConstant: Self.vowelButtonWidth).isActive = true
            button.addAction(UIAction { [weak self] _ in self?.onVowelSign?(sign) }, for: .touchUpInside)
            return button
        })
        // A new consonant starts the row from the beginning again; a reorder for
        // the same consonant leaves it where the user scrolled it.
        if base != stripBase { vowelScroll.setContentOffset(.zero, animated: false) }
        stripBase = base
        updateVisibility()
    }

    private func replaceItems(in stack: UIStackView, with views: [UIView]) {
        stack.arrangedSubviews.forEach { $0.removeFromSuperview() }
        views.forEach(stack.addArrangedSubview)
    }
}

/// A hold on one suggestion slot, carrying the slot it was made for.
private final class SlotHold: UILongPressGestureRecognizer {
    var slot: SuggestionSlot?
}
