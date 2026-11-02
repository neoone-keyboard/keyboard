import UIKit
import KeyboardCore

/// The strip above the keys: three suggestion slots, or after a Malayalam
/// consonant the best completion on the left and the vowel signs scrolling
/// sideways at full size beside it.
final class TopBarView: UIView {
    var onVowelSign: (@MainActor (String) -> Void)?
    var onSuggestion: (@MainActor (SuggestionSlot) -> Void)?

    private let suggestionStack = UIStackView()
    private let vowelRow = UIStackView()
    private let completionButton = UIButton(type: .custom)
    private var completion: Suggestion?
    private let vowelScroll = UIScrollView()
    private let vowelStack = UIStackView()
    private var theme: KeyboardTheme?
    /// What is showing now, so unchanged content isn't rebuilt on every keystroke.
    private var shownContent: [String] = []

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

        for view in [suggestionStack, vowelRow, vowelStack] {
            view.translatesAutoresizingMaskIntoConstraints = false
        }
        addSubview(suggestionStack)
        addSubview(vowelRow)
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
    }

    /// Left, middle, right. The middle slot is what space commits, so it is bold.
    func showSuggestions(_ bar: SuggestionBar) {
        let content = ["suggestions"] + bar.slots.map { slot in slot.map { "\($0.title)|\($0.quoted)" } ?? "" }
        guard content != shownContent else { return }
        shownContent = content
        vowelRow.isHidden = true
        suggestionStack.isHidden = false
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
            return button
        })
    }

    /// The best completion (if any) on the left, then one button per vowel
    /// sign drawn on `base` (കാ കി കീ …). Tapping a sign inserts the sign alone.
    func showVowelSigns(base: String, signs: [String], completion: Suggestion?) {
        let content = ["vowels", base, completion?.text ?? ""] + signs
        guard content != shownContent else { return }
        shownContent = content
        suggestionStack.isHidden = true
        vowelRow.isHidden = false
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
        // A new consonant starts the row from കാ again.
        vowelScroll.setContentOffset(.zero, animated: false)
    }

    private func replaceItems(in stack: UIStackView, with views: [UIView]) {
        stack.arrangedSubviews.forEach { $0.removeFromSuperview() }
        views.forEach(stack.addArrangedSubview)
    }
}
