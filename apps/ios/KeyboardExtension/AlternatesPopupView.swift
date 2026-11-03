import UIKit

/// The row of long-press options shown above a key.
final class AlternatesPopupView: UIView {
    private var labels: [UILabel] = []
    private var theme: KeyboardTheme?

    override init(frame: CGRect) {
        super.init(frame: frame)
        isUserInteractionEnabled = false
        isHidden = true
        layer.cornerRadius = 8
        layer.shadowOpacity = 0.3
        layer.shadowRadius = 4
        layer.shadowOffset = CGSize(width: 0, height: 2)
    }

    required init?(coder: NSCoder) { fatalError("init(coder:) is not used") }

    /// Shows `options` above `keyRect`, kept inside `bounds` horizontally, with option
    /// `anchor` straight above the key (lit). Returns the x where the first option starts,
    /// for KeyGesture.setAlternatesOrigin.
    func show(_ options: [String], anchor: Int = 0, above keyRect: CGRect, within bounds: CGRect, theme: KeyboardTheme) -> CGFloat {
        self.theme = theme
        labels.forEach { $0.removeFromSuperview() }
        labels = options.map { option in
            let label = UILabel()
            label.text = option
            label.textAlignment = .center
            label.font = .systemFont(ofSize: 24)
            label.layer.cornerRadius = 6
            label.clipsToBounds = true
            addSubview(label)
            return label
        }
        let width = keyRect.width * CGFloat(options.count)
        let x = min(max(keyRect.minX - CGFloat(anchor) * keyRect.width, bounds.minX), bounds.maxX - width)
        frame = CGRect(x: x, y: keyRect.minY - keyRect.height - 8, width: width, height: keyRect.height)
        for (index, label) in labels.enumerated() {
            label.frame = CGRect(x: CGFloat(index) * keyRect.width, y: 0, width: keyRect.width, height: keyRect.height)
                .insetBy(dx: 2, dy: 2)
        }
        backgroundColor = theme.key
        layer.shadowColor = theme.shadow.cgColor
        highlight(anchor)
        isHidden = false
        return x
    }

    func highlight(_ index: Int) {
        guard let theme else { return }
        for (i, label) in labels.enumerated() {
            label.backgroundColor = i == index ? theme.accent : .clear
            label.textColor = i == index ? theme.onAccent : theme.keyText
        }
    }

    func hide() {
        isHidden = true
    }
}
