import UIKit

/// One key: its label, the long-press hint in the corner, and a pressed state.
final class KeyCapView: UIView {
    var isPressed = false {
        didSet { applyColors() }
    }

    private let title = UILabel()
    private let hint = UILabel()
    private var theme: KeyboardTheme?
    private var isSpecial = false

    override init(frame: CGRect) {
        super.init(frame: frame)
        isUserInteractionEnabled = false
        layer.cornerRadius = 6
        layer.shadowOffset = CGSize(width: 0, height: 1)
        layer.shadowRadius = 0
        layer.shadowOpacity = 1
        title.textAlignment = .center
        title.adjustsFontSizeToFitWidth = true
        title.minimumScaleFactor = 0.5
        hint.font = .systemFont(ofSize: 10)
        hint.textAlignment = .right
        addSubview(title)
        addSubview(hint)
    }

    required init?(coder: NSCoder) { fatalError("init(coder:) is not used") }

    func configure(title text: String, hint hintText: String?, special: Bool, theme: KeyboardTheme) {
        title.text = text
        title.font = special ? .systemFont(ofSize: 16) : .systemFont(ofSize: 24)
        hint.text = hintText
        isSpecial = special
        self.theme = theme
        applyColors()
    }

    override func layoutSubviews() {
        super.layoutSubviews()
        title.frame = bounds.insetBy(dx: 3, dy: 2)
        hint.frame = CGRect(x: 2, y: 2, width: bounds.width - 6, height: 12)
    }

    private func applyColors() {
        guard let theme else { return }
        let normal = isSpecial ? theme.specialKey : theme.key
        let pressed = isSpecial ? theme.key : theme.specialKey
        backgroundColor = isPressed ? pressed : normal
        title.textColor = theme.keyText
        hint.textColor = theme.hintText
        layer.shadowColor = theme.shadow.cgColor
    }
}
