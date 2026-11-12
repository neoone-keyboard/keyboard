import UIKit
import KeyboardCore

/// The strip above the keys: the suggestions, or after a Malayalam consonant the best
/// completion on the left and the vowel signs scrolling sideways at full size beside it.
///
/// Suggestions are laid out as `BarFit` says: as many whole words as fit, the default (what
/// space commits) first at the leading edge and bold, then the rest in the engine's order, those
/// that don't fit past the trailing edge where a finger slides the bar to them. With more past
/// the edge, a small accent mark there (`moreHintView`) pulses briefly for each new set, then
/// stays still until the bar is slid (`moreHintEnabled`, the app's setting). Holding a word asks
/// whether to remove it (`SuggestionRemoval`).
final class TopBarView: UIView, UIScrollViewDelegate {
    var onVowelSign: (@MainActor (String) -> Void)?
    var onSuggestion: (@MainActor (SuggestionSlot) -> Void)?
    /// The user confirmed removing a word held in the bar.
    var onRemoveSuggestion: (@MainActor (String) -> Void)?

    /// Whether the trailing edge hints at suggestions past it (the app's "Hint when there are
    /// more suggestions", `SharedSettings.suggestionMoreHint`). Off, they still scroll.
    var moreHintEnabled = true {
        didSet {
            guard moreHintEnabled != oldValue else { return }
            shownContent = []
            layoutSuggestions(force: true)
        }
    }

    /// Scrolls the suggestion buttons, each placed by `fit`. As wide as the bar when all fit.
    private let suggestionScroll = SlotScrollView()
    /// The suggestion buttons, made as needed and reused for every set after.
    private var buttons: [UIButton] = []
    /// The words the buttons show now, in the bar's order (`SuggestionBar.ordered`).
    private var slots: [SuggestionSlot] = []
    /// Each shown word's text width at its font, from `widths`.
    private var textWidths: [Double] = []
    private var widths = TextWidthCache(capacity: 512)
    /// How many words have been measured: the cache's misses, for tests.
    private(set) var measuredWords = 0
    private var fit: BarFit?
    /// The width `fit` was made for; a new width lays the bar out again.
    private var fitWidth: CGFloat = -1
    /// A new set arrived: the hint pulses once it is laid out.
    private var pulsePending = false
    /// The accent mark at the trailing edge while words are past it.
    let moreHintView = MoreHintView()
    private static let pulseKey = "pulse"
    /// A short note in place of the words ("Swipe again").
    private let messageLabel = UILabel()

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
    /// "Remove “word”?" with Remove and Cancel, over the words while `removal` is set.
    private let confirmRow = UIStackView()
    private let confirmLabel = UILabel()
    private let removeButton = UIButton(type: .custom)
    private let cancelButton = UIButton(type: .custom)
    private var removal: SuggestionRemoval?
    private var removalTimeout: Task<Void, Never>?

    /// Whether the vowel strip is showing (rather than the suggestions).
    var showsVowelSigns: Bool { stripBase != nil }

    /// The pack whose conventions the bar follows (`InputController.scriptPack`): right to
    /// left, it is mirrored, and its script's words are drawn in its own font (`barFonts`).
    private var script: PackDescriptor?
    private var rightToLeft: Bool { script?.rightToLeft ?? false }

    /// Width of one vowel-sign button. Wide enough to tap without zooming;
    /// the last one is cut off at the edge, which shows the row scrolls.
    private static let vowelButtonWidth: CGFloat = 52
    private static let wordSize: CGFloat = 17
    private static let emojiSize: CGFloat = 24

    override init(frame: CGRect) {
        super.init(frame: frame)
        suggestionScroll.showsHorizontalScrollIndicator = false
        suggestionScroll.showsVerticalScrollIndicator = false
        suggestionScroll.alwaysBounceVertical = false
        suggestionScroll.contentInsetAdjustmentBehavior = .never
        // Taps on the words: no delay, and a slide that starts on a word scrolls the bar
        // instead of committing it (`SlotScrollView`).
        suggestionScroll.delaysContentTouches = false
        suggestionScroll.delegate = self
        for edge in [suggestionScroll.topEdgeEffect, suggestionScroll.bottomEdgeEffect,
                     suggestionScroll.leftEdgeEffect, suggestionScroll.rightEdgeEffect] {
            edge.isHidden = true
        }
        messageLabel.textAlignment = .center
        messageLabel.font = .systemFont(ofSize: 16)
        messageLabel.isHidden = true
        suggestionScroll.addSubview(messageLabel)
        moreHintView.isHidden = true

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

        for view in [suggestionScroll, vowelRow, vowelStack, confirmRow] {
            view.translatesAutoresizingMaskIntoConstraints = false
        }
        addSubview(suggestionScroll)
        addSubview(moreHintView)
        addSubview(vowelRow)
        addSubview(confirmRow)
        let content = vowelScroll.contentLayoutGuide
        NSLayoutConstraint.activate([
            suggestionScroll.leadingAnchor.constraint(equalTo: leadingAnchor),
            suggestionScroll.trailingAnchor.constraint(equalTo: trailingAnchor),
            suggestionScroll.topAnchor.constraint(equalTo: topAnchor),
            suggestionScroll.bottomAnchor.constraint(equalTo: bottomAnchor),
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
        messageLabel.textColor = theme.hintText
        moreHintView.color = theme.accent
        for button in buttons {
            button.setTitleColor(theme.keyText, for: .normal)
            button.setTitleColor(theme.hintText, for: .highlighted)
        }
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

    @objc private func slotHeld(_ hold: UILongPressGestureRecognizer) {
        guard hold.state == .began, let index = hold.view?.tag, slots.indices.contains(index) else { return }
        askToRemove(slots[index])
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
        updateHint(pulse: false)
    }

    /// Follows `pack`'s conventions from the next content shown: mirrored for a script written
    /// right to left, its words in its bar font. Nil: left to right, the system font.
    func setScript(_ pack: PackDescriptor?) {
        guard pack?.id != script?.id || pack?.rightToLeft != script?.rightToLeft
        else { return }
        script = pack
        shownContent = []
    }

    // MARK: Suggestions

    /// The bar's words (`SuggestionBar.ordered`), as many as fit in view, the default first
    /// and bold. Right to left, the default is at the right edge and the rest read leftward.
    /// New words start the bar from its beginning again.
    func showSuggestions(_ bar: SuggestionBar) {
        let content = ["suggestions", "\(rightToLeft)", "\(bounds.height)", "\(moreHintEnabled)"]
            + bar.ordered.map { "\($0.title)|\($0.quoted)" }
        guard content != shownContent else { return }
        let sameWords = shownContent.dropFirst(4) == content.dropFirst(4)
        shownContent = content
        stripBase = nil
        messageLabel.isHidden = true
        slots = bar.ordered
        while buttons.count < slots.count { buttons.append(makeButton(tag: buttons.count)) }
        textWidths.removeAll(keepingCapacity: true)
        for (index, button) in buttons.enumerated() {
            guard index < slots.count else {
                button.isHidden = true
                button.setTitle(nil, for: .normal)
                continue
            }
            let font = configure(button, for: slots[index], isDefault: index == 0)
            textWidths.append(measure(slots[index].title, font: font))
        }
        // A setting or a script changed, but not the words: no new pulse.
        pulsePending = !sameWords || pulsePending
        updateVisibility()
        layoutSuggestions(force: true)
    }

    override func layoutSubviews() {
        super.layoutSubviews()
        layoutSuggestions(force: false)
    }

    /// Places the buttons as `BarFit` says for the bar's width. Only when the words or the
    /// width changed (not as the bar scrolls), and with nothing new made.
    private func layoutSuggestions(force: Bool) {
        let width = bounds.width
        guard width > 0, force || width != fitWidth, messageLabel.isHidden else { return }
        fitWidth = width
        let fit = BarFit(textWidths: textWidths, barWidth: width, rightToLeft: rightToLeft, hintEnabled: moreHintEnabled)
        self.fit = fit
        let height = bounds.height
        for (index, item) in fit.items.enumerated() {
            let button = buttons[index]
            button.frame = CGRect(x: item.x, y: 0, width: item.width, height: height)
            if index == 0, let font = button.titleLabel?.font {
                // Only ever the first, when it alone is wider than the bar.
                let size = baseSize(for: slots[0], font: font) * fit.firstScale
                if font.pointSize != size { button.titleLabel?.font = font.withSize(size) }
            }
        }
        suggestionScroll.contentSize = CGSize(width: fit.contentWidth, height: height)
        suggestionScroll.contentOffset = CGPoint(x: fit.startOffset, y: 0)
        updateHint(pulse: pulsePending)
        pulsePending = false
    }

    /// The size a word is drawn at before any shrinking.
    private func baseSize(for slot: SuggestionSlot, font: UIFont) -> CGFloat {
        if slot.suggestion.kind == .emoji { return Self.emojiSize }
        if let script, script.writes(slot.suggestion.text), Self.barFonts[script.id] != nil,
           let scriptFont = scriptFont(bold: false) {
            return scriptFont.pointSize
        }
        return Self.wordSize
    }

    /// Shows the hint while words are past the edge and the setting is on; `pulse` pulses it.
    private func updateHint(pulse: Bool) {
        let shows = fit?.showsHint == true && messageLabel.isHidden && !suggestionScroll.isHidden
            && abs(suggestionScroll.contentOffset.x - (fit?.startOffset ?? 0)) < 2
        guard shows else {
            moreHintView.isHidden = true
            moreHintView.layer.removeAnimation(forKey: Self.pulseKey)
            return
        }
        let hintWidth = BarFit.Spacing().hintWidth
        moreHintView.rightToLeft = rightToLeft
        moreHintView.frame = CGRect(x: rightToLeft ? 0 : bounds.width - hintWidth, y: 0, width: hintWidth, height: bounds.height)
        moreHintView.isHidden = false
        guard pulse, !UIAccessibility.isReduceMotionEnabled else { return }
        // Twice brighter and back, then still: one short animation on the one layer.
        let animation = CAKeyframeAnimation(keyPath: "opacity")
        animation.values = [0, 1, 0.35, 1, 0.35, 1]
        animation.keyTimes = [0, 0.2, 0.4, 0.6, 0.8, 1]
        animation.duration = 1.6
        animation.timingFunction = CAMediaTimingFunction(name: .easeInEaseOut)
        moreHintView.layer.add(animation, forKey: Self.pulseKey)
    }

    func scrollViewDidScroll(_ scrollView: UIScrollView) {
        // Slid: the hint has done its job until the next set.
        guard !moreHintView.isHidden, let fit, abs(scrollView.contentOffset.x - fit.startOffset) >= 2 else { return }
        updateHint(pulse: false)
    }

    /// A reusable button for the word at `tag` in `slots`: a tap reports it (`onSuggestion`),
    /// a hold asks to remove it.
    private func makeButton(tag: Int) -> UIButton {
        let button = UIButton(type: .custom)
        button.tag = tag
        button.titleLabel?.lineBreakMode = .byClipping
        button.titleLabel?.adjustsFontSizeToFitWidth = true
        button.titleLabel?.minimumScaleFactor = 0.5
        button.setTitleColor(theme?.keyText, for: .normal)
        button.setTitleColor(theme?.hintText, for: .highlighted)
        button.addAction(UIAction { [weak self, weak button] _ in
            guard let self, let index = button?.tag, slots.indices.contains(index) else { return }
            onSuggestion?(slots[index])
        }, for: .touchUpInside)
        // Once the hold is recognised the button's touch is cancelled, so it doesn't commit.
        let hold = UILongPressGestureRecognizer(target: self, action: #selector(slotHeld(_:)))
        hold.minimumPressDuration = SuggestionRemoval.holdDuration
        button.addGestureRecognizer(hold)
        suggestionScroll.addSubview(button)
        return button
    }

    /// Sets `button` up for `slot` and returns the font its title is drawn in.
    private func configure(_ button: UIButton, for slot: SuggestionSlot, isDefault: Bool) -> UIFont {
        button.isHidden = false
        button.setTitle(slot.title, for: .normal)
        // The word, not its drawn title (which may be in quotes).
        button.accessibilityLabel = slot.suggestion.text
        button.accessibilityLanguage = nil
        var font = UIFont.systemFont(ofSize: Self.wordSize, weight: isDefault ? .semibold : .regular)
        if let script, script.writes(slot.suggestion.text) {
            if let scriptFont = scriptFont(bold: isDefault) { font = scriptFont }
            button.accessibilityLanguage = script.id
        }
        let emoji = slot.suggestion.kind == .emoji
        // An emoji is drawn larger, and has no word to remove.
        if emoji { font = .systemFont(ofSize: Self.emojiSize) }
        button.titleLabel?.font = font
        button.gestureRecognizers?.forEach { $0.isEnabled = !emoji }
        button.accessibilityCustomActions = emoji ? nil : [
            UIAccessibilityCustomAction(name: "Remove suggestion") { [weak self] _ in
                self?.askToRemove(slot)
                return true
            },
        ]
        return font
    }

    /// `text`'s width in `font`, measured once per word and font (`widths`).
    private func measure(_ text: String, font: UIFont) -> Double {
        let key = "\(font.fontName)|\(font.pointSize)|\(text)"
        return widths.width(of: key) { _ in
            measuredWords += 1
            return Double((text as NSString).size(withAttributes: [.font: font]).width.rounded(.up))
        }
    }

    /// The font family a language's words are drawn in when its writing has a style of its own
    /// that the system font doesn't draw: Urdu's Nastaliq (the system draws Arabic letters in
    /// Naskh, which the keys and popups keep). iOS has Noto Nastaliq Urdu, which a keyboard
    /// extension can use like any system font. By language id, as Android's `ScriptFonts`.
    static let barFonts = ["ur": "Noto Nastaliq Urdu"]

    /// The language's bar font (`barFonts`) at the size the bar's
    /// words have, made smaller if its line would be taller than the bar: Nastaliq's tall
    /// ascenders and deep descenders need about two and a half times its size, and a line
    /// that doesn't fit is clipped. `bold` takes the family's Bold face (iOS has
    /// NotoNastaliqUrdu-Bold), so the default looks heavier, as in English. Nil when the
    /// phone hasn't got the font.
    private func scriptFont(bold: Bool) -> UIFont? {
        guard let id = script?.id, let family = Self.barFonts[id], UIFont.familyNames.contains(family) else { return nil }
        var descriptor = UIFontDescriptor(fontAttributes: [.family: family])
        if bold, let heavier = descriptor.withSymbolicTraits(.traitBold) { descriptor = heavier }
        let size = Self.wordSize
        let font = UIFont(descriptor: descriptor, size: size)
        let room = bounds.height > 0 ? bounds.height : KeyboardHeight.portraitBar
        guard font.lineHeight > room else { return font }
        return UIFont(descriptor: descriptor, size: (size * room / font.lineHeight * 2).rounded(.down) / 2)
    }

    /// A short note in the middle of the bar, in place of the words ("Swipe again").
    func showMessage(_ text: String) {
        let content = ["message", text]
        guard content != shownContent else { return }
        shownContent = content
        stripBase = nil
        slots = []
        textWidths.removeAll(keepingCapacity: true)
        fit = nil
        buttons.forEach { $0.isHidden = true; $0.setTitle(nil, for: .normal) }
        messageLabel.text = text
        messageLabel.isHidden = false
        messageLabel.frame = bounds
        messageLabel.autoresizingMask = [.flexibleWidth, .flexibleHeight]
        suggestionScroll.contentSize = bounds.size
        suggestionScroll.contentOffset = .zero
        updateVisibility()
    }

    // MARK: Vowel signs

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
        vowelStack.arrangedSubviews.forEach { $0.removeFromSuperview() }
        for sign in signs {
            let button = UIButton(type: .system)
            button.setTitle(base + sign, for: .normal)
            button.titleLabel?.font = .systemFont(ofSize: 20)
            button.setTitleColor(theme?.onAccent, for: .normal)
            button.backgroundColor = theme?.accent
            button.layer.cornerRadius = 6
            button.widthAnchor.constraint(equalToConstant: Self.vowelButtonWidth).isActive = true
            button.addAction(UIAction { [weak self] _ in self?.onVowelSign?(sign) }, for: .touchUpInside)
            vowelStack.addArrangedSubview(button)
        }
        // A new consonant starts the row from the beginning again; a reorder for
        // the same consonant leaves it where the user scrolled it.
        if base != stripBase { vowelScroll.setContentOffset(.zero, animated: false) }
        stripBase = base
        updateVisibility()
    }
}

/// The suggestion bar's scroll view: a slide that starts on a word scrolls, as one that
/// starts between words does (a plain scroll view leaves a touch on a control alone).
private final class SlotScrollView: UIScrollView {
    override func touchesShouldCancel(in view: UIView) -> Bool { true }
}

/// The hint at the bar's trailing edge that more words are past it: a soft accent glow
/// fading in toward the edge, with a small chevron pointing the way the bar slides. Its own
/// layer is the glow, so one animation on it pulses the whole mark. Touches pass through.
final class MoreHintView: UIView {
    override class var layerClass: AnyClass { CAGradientLayer.self }
    private var glow: CAGradientLayer { layer as! CAGradientLayer }
    private let chevron = CAShapeLayer()

    var color: UIColor = .systemBlue { didSet { setNeedsLayout() } }
    var rightToLeft = false { didSet { if rightToLeft != oldValue { setNeedsLayout() } } }

    override init(frame: CGRect) {
        super.init(frame: frame)
        isUserInteractionEnabled = false
        isAccessibilityElement = false
        chevron.fillColor = nil
        chevron.lineWidth = 2
        chevron.lineCap = .round
        chevron.lineJoin = .round
        layer.addSublayer(chevron)
    }

    required init?(coder: NSCoder) { fatalError("init(coder:) is not used") }

    override func layoutSubviews() {
        super.layoutSubviews()
        let resolved = color.resolvedColor(with: traitCollection)
        glow.colors = [resolved.withAlphaComponent(0).cgColor, resolved.withAlphaComponent(0.28).cgColor]
        glow.startPoint = CGPoint(x: rightToLeft ? 1 : 0, y: 0.5)
        glow.endPoint = CGPoint(x: rightToLeft ? 0 : 1, y: 0.5)
        chevron.strokeColor = resolved.cgColor
        // 5 by 10 points, centred, pointing at the edge.
        let mid = CGPoint(x: bounds.midX + (rightToLeft ? -2 : 2), y: bounds.midY)
        let path = UIBezierPath()
        let back: CGFloat = rightToLeft ? 2.5 : -2.5
        path.move(to: CGPoint(x: mid.x + back, y: mid.y - 5))
        path.addLine(to: CGPoint(x: mid.x - back, y: mid.y))
        path.addLine(to: CGPoint(x: mid.x + back, y: mid.y + 5))
        CATransaction.begin()
        CATransaction.setDisableActions(true)
        chevron.frame = bounds
        chevron.path = path.cgPath
        CATransaction.commit()
    }
}
