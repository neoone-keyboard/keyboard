import UIKit

/// The strip above the keys: suggestions in English, vowel signs after a Malayalam consonant.
final class TopBarView: UIStackView {
    var onVowelSign: (@MainActor (String) -> Void)?

    private var theme: KeyboardTheme?
    /// What is showing now, so unchanged content isn't rebuilt on every keystroke.
    private var shownContent: [String] = []

    override init(frame: CGRect) {
        super.init(frame: frame)
        axis = .horizontal
        distribution = .fillEqually
        spacing = 3
    }

    required init(coder: NSCoder) { fatalError("init(coder:) is not used") }

    func apply(_ theme: KeyboardTheme) {
        self.theme = theme
        shownContent = []
    }

    /// `suggestions[0]` is the best and goes in the middle slot.
    func showSuggestions(_ suggestions: [String]) {
        let content = ["suggestions"] + suggestions
        guard content != shownContent else { return }
        shownContent = content
        replaceItems(with: [1, 0, 2].map { index in
            let label = UILabel()
            label.text = index < suggestions.count ? suggestions[index] : nil
            label.textAlignment = .center
            label.font = .systemFont(ofSize: 17, weight: index == 0 ? .semibold : .regular)
            label.textColor = theme?.keyText
            label.adjustsFontSizeToFitWidth = true
            label.minimumScaleFactor = 0.6
            return label
        })
    }

    /// One button per vowel sign, drawn on `base` (കാ കി കീ …). Tapping inserts the sign alone.
    func showVowelSigns(base: String, signs: [String]) {
        let content = ["vowels", base] + signs
        guard content != shownContent else { return }
        shownContent = content
        replaceItems(with: signs.map { sign in
            let button = UIButton(type: .system)
            button.setTitle(base + sign, for: .normal)
            button.titleLabel?.font = .systemFont(ofSize: 18)
            button.titleLabel?.adjustsFontSizeToFitWidth = true
            button.titleLabel?.minimumScaleFactor = 0.6
            button.setTitleColor(theme?.onAccent, for: .normal)
            button.backgroundColor = theme?.accent
            button.layer.cornerRadius = 6
            button.addAction(UIAction { [weak self] _ in self?.onVowelSign?(sign) }, for: .touchUpInside)
            return button
        })
    }

    private func replaceItems(with views: [UIView]) {
        arrangedSubviews.forEach { $0.removeFromSuperview() }
        views.forEach(addArrangedSubview)
    }
}
