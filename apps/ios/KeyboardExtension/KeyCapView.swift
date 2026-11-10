import UIKit
import KeyboardCore

/// One key: its label, the long-press hint in the corner, and a pressed state.
final class KeyCapView: UIView {
    var isPressed = false {
        didSet { applyColors() }
    }

    private let title = UILabel()
    private let hint = UILabel()
    /// An SF Symbol in the title's place (the emoji key's smiley).
    private let icon = UIImageView()
    private var theme: KeyboardTheme?
    private var isSpecial = false
    /// The label's size on a full-height key; landscape's short keys shrink it to fit.
    private var fontSize: CGFloat = 24
    private var fittedFontSize: CGFloat {
        bounds.height > 0 ? min(fontSize, (bounds.height * 0.8).rounded(.down)) : fontSize
    }
    /// VoiceOver typed the key (in touch typing, the finger lifted on it; or a double-tap).
    var onActivate: (@MainActor () -> Void)?

    override init(frame: CGRect) {
        super.init(frame: frame)
        isUserInteractionEnabled = false
        // One element per key, read as it types; with `.keyboardKey` VoiceOver can type on
        // lift (touch typing), as on the system keyboard. The drawn labels inside are hidden.
        isAccessibilityElement = true
        accessibilityTraits = .keyboardKey
        layer.cornerRadius = 6
        layer.shadowOffset = CGSize(width: 0, height: 1)
        layer.shadowRadius = 0
        layer.shadowOpacity = 1
        title.textAlignment = .center
        title.adjustsFontSizeToFitWidth = true
        title.minimumScaleFactor = 0.5
        hint.font = .systemFont(ofSize: 10)
        hint.textAlignment = .right
        icon.contentMode = .center
        addSubview(title)
        addSubview(hint)
        addSubview(icon)
    }

    required init?(coder: NSCoder) { fatalError("init(coder:) is not used") }

    func configure(title text: String, hint hintText: String?, special: Bool, symbol: String? = nil, theme: KeyboardTheme) {
        title.text = symbol == nil ? text : nil
        fontSize = special ? 16 : 24
        title.font = .systemFont(ofSize: fittedFontSize)
        icon.image = symbol.flatMap { UIImage(systemName: $0, withConfiguration: UIImage.SymbolConfiguration(pointSize: 18)) }
        hint.text = hintText
        isSpecial = special
        self.theme = theme
        applyColors()
    }

    /// What VoiceOver says for the key (`InputController.spokenKey`), and its long-press
    /// choices as custom actions. `identifier` is the drawn label, for UI tests.
    func configureAccessibility(_ spoken: SpokenKey, identifier: String, actions: [UIAccessibilityCustomAction]) {
        accessibilityLabel = spoken.label
        accessibilityValue = spoken.value
        accessibilityIdentifier = identifier
        accessibilityCustomActions = actions
    }

    override func accessibilityActivate() -> Bool {
        guard let onActivate else { return false }
        onActivate()
        return true
    }

    override func layoutSubviews() {
        super.layoutSubviews()
        title.frame = bounds.insetBy(dx: 3, dy: 2)
        if title.font.pointSize != fittedFontSize { title.font = .systemFont(ofSize: fittedFontSize) }
        hint.frame = CGRect(x: 2, y: 2, width: bounds.width - 6, height: 12)
        icon.frame = bounds
    }

    private func applyColors() {
        guard let theme else { return }
        let normal = isSpecial ? theme.specialKey : theme.key
        let pressed = isSpecial ? theme.key : theme.specialKey
        backgroundColor = isPressed ? pressed : normal
        title.textColor = theme.keyText
        icon.tintColor = theme.keyText
        hint.textColor = theme.hintText
        layer.shadowColor = theme.shadow.cgColor
    }
}
