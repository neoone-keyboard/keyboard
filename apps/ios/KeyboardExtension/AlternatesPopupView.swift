import UIKit

/// The long-press options shown above a key, in one row or several.
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

    /// Shows `options` above `keyRect` in rows of `columns` (top row first), kept inside
    /// `bounds` horizontally, with option `anchor` straight above the key (lit). Returns
    /// the x where the first column starts, for KeyGesture.setAlternatesOrigin.
    func show(
        _ options: [String], columns: Int? = nil, anchor: Int = 0,
        above keyRect: CGRect, within bounds: CGRect, theme: KeyboardTheme
    ) -> CGFloat {
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
        let columns = max(1, min(columns ?? options.count, options.count))
        let rows = (options.count + columns - 1) / columns
        let width = keyRect.width * CGFloat(columns)
        let height = keyRect.height * CGFloat(rows)
        let x = min(max(keyRect.minX - CGFloat(anchor % columns) * keyRect.width, bounds.minX), bounds.maxX - width)
        frame = CGRect(x: x, y: keyRect.minY - height - 8, width: width, height: height)
        for (index, label) in labels.enumerated() {
            label.frame = CGRect(
                x: CGFloat(index % columns) * keyRect.width, y: CGFloat(index / columns) * keyRect.height,
                width: keyRect.width, height: keyRect.height
            ).insetBy(dx: 2, dy: 2)
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
