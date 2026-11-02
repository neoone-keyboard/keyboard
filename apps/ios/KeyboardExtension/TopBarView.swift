import UIKit

/// The strip above the keys: suggestions in English, vowel signs after a Malayalam consonant.
/// Suggestions sit in three fixed slots; vowel signs scroll sideways at full size.
final class TopBarView: UIView {
    var onVowelSign: (@MainActor (String) -> Void)?
    var onSuggestion: (@MainActor (String) -> Void)?

    private let suggestionStack = UIStackView()
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

        for view in [suggestionStack, vowelScroll, vowelStack] {
            view.translatesAutoresizingMaskIntoConstraints = false
        }
        addSubview(suggestionStack)
        addSubview(vowelScroll)
        let content = vowelScroll.contentLayoutGuide
        NSLayoutConstraint.activate([
            suggestionStack.leadingAnchor.constraint(equalTo: leadingAnchor),
            suggestionStack.trailingAnchor.constraint(equalTo: trailingAnchor),
            suggestionStack.topAnchor.constraint(equalTo: topAnchor),
            suggestionStack.bottomAnchor.constraint(equalTo: bottomAnchor),
            vowelScroll.leadingAnchor.constraint(equalTo: leadingAnchor),
            vowelScroll.trailingAnchor.constraint(equalTo: trailingAnchor),
            vowelScroll.topAnchor.constraint(equalTo: topAnchor),
            vowelScroll.bottomAnchor.constraint(equalTo: bottomAnchor),
            vowelStack.leadingAnchor.constraint(equalTo: content.leadingAnchor),
            vowelStack.trailingAnchor.constraint(equalTo: content.trailingAnchor),
            vowelStack.topAnchor.constraint(equalTo: content.topAnchor),
            vowelStack.bottomAnchor.constraint(equalTo: content.bottomAnchor),
            vowelStack.heightAnchor.constraint(equalTo: vowelScroll.frameLayoutGuide.heightAnchor),
        ])
        vowelScroll.isHidden = true
    }

    required init?(coder: NSCoder) { fatalError("init(coder:) is not used") }

    func apply(_ theme: KeyboardTheme) {
        self.theme = theme
        shownContent = []
    }

    /// `suggestions[0]` is the best and goes in the middle slot.
    func showSuggestions(_ suggestions: [String]) {
        let content = ["suggestions"] + suggestions
        guard content != shownContent else { return }
        shownContent = content
        vowelScroll.isHidden = true
        suggestionStack.isHidden = false
        replaceItems(in: suggestionStack, with: [1, 0, 2].map { index in
            let button = UIButton(type: .custom)
            guard index < suggestions.count else { return button }
            let suggestion = suggestions[index]
            button.setTitle(suggestion, for: .normal)
            button.titleLabel?.font = .systemFont(ofSize: 17, weight: index == 0 ? .semibold : .regular)
            button.titleLabel?.adjustsFontSizeToFitWidth = true
            button.titleLabel?.minimumScaleFactor = 0.6
            button.setTitleColor(theme?.keyText, for: .normal)
            button.setTitleColor(theme?.hintText, for: .highlighted)
            button.addAction(UIAction { [weak self] _ in self?.onSuggestion?(suggestion) }, for: .touchUpInside)
            return button
        })
    }

    /// One button per vowel sign, drawn on `base` (കാ കി കീ …). Tapping inserts the sign alone.
    func showVowelSigns(base: String, signs: [String]) {
        let content = ["vowels", base] + signs
        guard content != shownContent else { return }
        shownContent = content
        suggestionStack.isHidden = true
        vowelScroll.isHidden = false
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
