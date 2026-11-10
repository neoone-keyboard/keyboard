import UIKit
import KeyboardCore

/// The strip above the keys: three suggestion slots, or after a Malayalam
/// consonant the best completion on the left and the vowel signs scrolling
/// sideways at full size beside it. Holding a suggestion slot asks whether to
/// remove its word (`SuggestionRemoval`). Suggestions past the three (`SuggestionBar.more`)
/// follow the right slot, each as wide as its word, and a finger slides the bar to them.
final class TopBarView: UIView {
    var onVowelSign: (@MainActor (String) -> Void)?
    var onSuggestion: (@MainActor (SuggestionSlot) -> Void)?
    /// The user confirmed removing a word held in the bar.
    var onRemoveSuggestion: (@MainActor (String) -> Void)?

    private let suggestionStack = UIStackView()
    /// Scrolls the three slots, which fill its width as they always have, and `moreStack`
    /// after them. With nothing more it is exactly as wide as the bar and doesn't move.
    private let suggestionScroll = SlotScrollView()
    private let moreStack = UIStackView()
    /// Holds `moreStack` at zero width while it has nothing in it.
    private var moreCollapsed: NSLayoutConstraint!
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
    /// Space on each side of a word past the three slots, and the narrowest one may be.
    private static let morePadding: CGFloat = 12
    private static let moreMinimumWidth: CGFloat = 44

    override init(frame: CGRect) {
        super.init(frame: frame)
        suggestionStack.axis = .horizontal
        suggestionStack.distribution = .fillEqually
        suggestionStack.spacing = 3
        moreStack.axis = .horizontal
        moreStack.spacing = 3
        // Its leading edge is the space after the right slot, as between the slots.
        moreStack.isLayoutMarginsRelativeArrangement = true
        moreStack.directionalLayoutMargins = NSDirectionalEdgeInsets(top: 0, leading: 3, bottom: 0, trailing: 0)
        suggestionScroll.showsHorizontalScrollIndicator = false
        suggestionScroll.showsVerticalScrollIndicator = false
        // Taps on the slots behave as before: no delay, and a slide that starts on a slot
        // scrolls the bar instead of committing it (`SlotScrollView`).
        suggestionScroll.delaysContentTouches = false
        for edge in [suggestionScroll.topEdgeEffect, suggestionScroll.bottomEdgeEffect,
                     suggestionScroll.leftEdgeEffect, suggestionScroll.rightEdgeEffect] {
            edge.isHidden = true
        }
        suggestionScroll.addSubview(suggestionStack)
        suggestionScroll.addSubview(moreStack)

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

        for view in [suggestionScroll, suggestionStack, moreStack, vowelRow, vowelStack, confirmRow] {
            view.translatesAutoresizingMaskIntoConstraints = false
        }
        addSubview(suggestionScroll)
        addSubview(vowelRow)
        addSubview(confirmRow)
        let content = vowelScroll.contentLayoutGuide
        let slotsContent = suggestionScroll.contentLayoutGuide
        let slotsFrame = suggestionScroll.frameLayoutGuide
        moreCollapsed = moreStack.widthAnchor.constraint(equalToConstant: 0)
        NSLayoutConstraint.activate([
            suggestionScroll.leadingAnchor.constraint(equalTo: leadingAnchor),
            suggestionScroll.trailingAnchor.constraint(equalTo: trailingAnchor),
            suggestionScroll.topAnchor.constraint(equalTo: topAnchor),
            suggestionScroll.bottomAnchor.constraint(equalTo: bottomAnchor),
            suggestionStack.leadingAnchor.constraint(equalTo: slotsContent.leadingAnchor),
            suggestionStack.topAnchor.constraint(equalTo: slotsContent.topAnchor),
            suggestionStack.bottomAnchor.constraint(equalTo: slotsContent.bottomAnchor),
            suggestionStack.widthAnchor.constraint(equalTo: slotsFrame.widthAnchor),
            suggestionStack.heightAnchor.constraint(equalTo: slotsFrame.heightAnchor),
            moreStack.leadingAnchor.constraint(equalTo: suggestionStack.trailingAnchor),
            moreStack.trailingAnchor.constraint(equalTo: slotsContent.trailingAnchor),
            moreStack.topAnchor.constraint(equalTo: slotsContent.topAnchor),
            moreStack.bottomAnchor.constraint(equalTo: slotsContent.bottomAnchor),
            moreCollapsed,
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
        suggestionScroll.isHidden = asking || stripBase != nil
        vowelRow.isHidden = asking || stripBase == nil
    }

    /// Left, middle, right. The middle slot is what space commits, so it is bold. Then the
    /// bar's `more`, past the right edge until the bar is slid. New content starts the bar
    /// from the beginning again.
    func showSuggestions(_ bar: SuggestionBar) {
        let content = ["suggestions"] + (bar.slots + bar.more.map(Optional.some)).map { slot in
            slot.map { "\($0.title)|\($0.quoted)" } ?? ""
        }
        guard content != shownContent else { return }
        shownContent = content
        stripBase = nil
        updateVisibility()
        replaceItems(in: suggestionStack, with: bar.slots.enumerated().map { index, slot in
            guard let slot else {
                // An empty slot is nothing to VoiceOver.
                let empty = UIButton(type: .custom)
                empty.isAccessibilityElement = false
                return empty
            }
            return slotButton(slot, bold: index == 1)
        })
        showMore(bar.more.map { slot in
            let button = slotButton(slot, bold: false)
            button.titleLabel?.adjustsFontSizeToFitWidth = false
            if let title = button.titleLabel {
                let fit = button.widthAnchor.constraint(equalTo: title.widthAnchor, constant: 2 * Self.morePadding)
                fit.priority = .defaultHigh
                fit.isActive = true
            }
            button.widthAnchor.constraint(greaterThanOrEqualToConstant: Self.moreMinimumWidth).isActive = true
            return button
        })
    }

    /// The words past the three slots; none collapses the scrolled area to the bar's width.
    private func showMore(_ buttons: [UIView]) {
        replaceItems(in: moreStack, with: buttons)
        moreCollapsed.isActive = buttons.isEmpty
        suggestionScroll.setContentOffset(.zero, animated: false)
    }

    /// One suggestion's button: a tap reports `slot` (`onSuggestion`), a hold asks to remove its word.
    private func slotButton(_ slot: SuggestionSlot, bold: Bool) -> UIButton {
        let button = UIButton(type: .custom)
        button.setTitle(slot.title, for: .normal)
        // The word, not its drawn title (which may be in quotes).
        button.accessibilityLabel = slot.suggestion.text
        button.titleLabel?.font = .systemFont(ofSize: 17, weight: bold ? .semibold : .regular)
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
        showMore([])
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

/// The suggestion bar's scroll view: a slide that starts on a slot scrolls, as one that
/// starts between slots does (a plain scroll view leaves a touch on a control alone).
private final class SlotScrollView: UIScrollView {
    override func touchesShouldCancel(in view: UIView) -> Bool { true }
}

/// A hold on one suggestion slot, carrying the slot it was made for.
private final class SlotHold: UILongPressGestureRecognizer {
    var slot: SuggestionSlot?
}
