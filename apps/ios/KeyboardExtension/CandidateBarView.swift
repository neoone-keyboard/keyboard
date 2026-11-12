import UIKit
import KeyboardCore

/// The bar while a phrase-conversion language (Japanese) is typed, in the suggestion bar's
/// place: the candidates best first, sliding sideways, the one shown in the marked text
/// highlighted; while converting, arrows at the left that shrink and grow the first segment;
/// and an arrow at the right end that opens every candidate in place of the keys.
final class CandidateBarView: UIView {
    var onChoose: (@MainActor (Int) -> Void)?
    var onShrink: (@MainActor () -> Void)?
    var onGrow: (@MainActor () -> Void)?
    var onExpand: (@MainActor () -> Void)?

    private let segmentStack = UIStackView()
    private let shrinkButton = UIButton(type: .system)
    private let growButton = UIButton(type: .system)
    private let scroll = UIScrollView()
    private let candidateStack = UIStackView()
    private let expandButton = UIButton(type: .system)
    private var theme: KeyboardTheme?
    private var shown: [String] = []
    private var shownHighlight: Int?

    private static let padding: CGFloat = 12
    private static let arrowWidth: CGFloat = 36

    override init(frame: CGRect) {
        super.init(frame: frame)
        for (button, symbol, label) in [(shrinkButton, "chevron.left", "shorter segment"),
                                        (growButton, "chevron.right", "longer segment"),
                                        (expandButton, "chevron.down", "all candidates")] {
            button.setImage(UIImage(systemName: symbol, withConfiguration: UIImage.SymbolConfiguration(pointSize: 16, weight: .semibold)), for: .normal)
            button.accessibilityLabel = label
            button.widthAnchor.constraint(equalToConstant: Self.arrowWidth).isActive = true
        }
        shrinkButton.addAction(UIAction { [weak self] _ in self?.onShrink?() }, for: .touchUpInside)
        growButton.addAction(UIAction { [weak self] _ in self?.onGrow?() }, for: .touchUpInside)
        expandButton.addAction(UIAction { [weak self] _ in self?.onExpand?() }, for: .touchUpInside)
        segmentStack.axis = .horizontal
        segmentStack.addArrangedSubview(shrinkButton)
        segmentStack.addArrangedSubview(growButton)

        candidateStack.axis = .horizontal
        candidateStack.spacing = 2
        scroll.showsHorizontalScrollIndicator = false
        scroll.showsVerticalScrollIndicator = false
        scroll.delaysContentTouches = false
        for edge in [scroll.topEdgeEffect, scroll.bottomEdgeEffect, scroll.leftEdgeEffect, scroll.rightEdgeEffect] {
            edge.isHidden = true
        }
        scroll.addSubview(candidateStack)

        let row = UIStackView(arrangedSubviews: [segmentStack, scroll, expandButton])
        row.axis = .horizontal
        row.spacing = 2
        for view in [row, candidateStack] { view.translatesAutoresizingMaskIntoConstraints = false }
        addSubview(row)
        let content = scroll.contentLayoutGuide
        NSLayoutConstraint.activate([
            row.leadingAnchor.constraint(equalTo: leadingAnchor),
            row.trailingAnchor.constraint(equalTo: trailingAnchor),
            row.topAnchor.constraint(equalTo: topAnchor),
            row.bottomAnchor.constraint(equalTo: bottomAnchor),
            candidateStack.leadingAnchor.constraint(equalTo: content.leadingAnchor),
            candidateStack.trailingAnchor.constraint(equalTo: content.trailingAnchor),
            candidateStack.topAnchor.constraint(equalTo: content.topAnchor),
            candidateStack.bottomAnchor.constraint(equalTo: content.bottomAnchor),
            candidateStack.heightAnchor.constraint(equalTo: scroll.frameLayoutGuide.heightAnchor),
        ])
        segmentStack.isHidden = true
        expandButton.isHidden = true
    }

    required init?(coder: NSCoder) { fatalError("init(coder:) is not used") }

    func apply(_ theme: KeyboardTheme) {
        self.theme = theme
        for button in [shrinkButton, growButton, expandButton] { button.tintColor = theme.keyText }
        shown = []
    }

    /// The composition's candidates; nil, or none, empties the bar.
    func show(_ phrase: PhraseInput?) {
        let candidates = phrase?.candidates.map(\.text) ?? []
        let marked = phrase?.highlighted
        segmentStack.isHidden = !(phrase?.isConverting ?? false)
        shrinkButton.isEnabled = phrase?.canShrink ?? false
        growButton.isEnabled = phrase?.canGrow ?? false
        expandButton.isHidden = candidates.isEmpty
        if candidates == shown {
            if marked != shownHighlight { highlight(marked) }
            return
        }
        shown = candidates
        shownHighlight = nil
        candidateStack.arrangedSubviews.forEach { $0.removeFromSuperview() }
        for (index, text) in candidates.enumerated() {
            candidateStack.addArrangedSubview(button(text, index: index))
        }
        scroll.setContentOffset(.zero, animated: false)
        highlight(marked)
    }

    private func button(_ text: String, index: Int) -> UIButton {
        let button = UIButton(type: .custom)
        button.setTitle(text, for: .normal)
        button.titleLabel?.font = .systemFont(ofSize: 18, weight: index == 0 ? .semibold : .regular)
        button.setTitleColor(theme?.keyText, for: .normal)
        button.setTitleColor(theme?.hintText, for: .highlighted)
        button.accessibilityLanguage = "ja"
        button.layer.cornerRadius = 6
        if let title = button.titleLabel {
            button.widthAnchor.constraint(greaterThanOrEqualTo: title.widthAnchor, constant: 2 * Self.padding).isActive = true
        }
        button.widthAnchor.constraint(greaterThanOrEqualToConstant: 40).isActive = true
        button.addAction(UIAction { [weak self] _ in self?.onChoose?(index) }, for: .touchUpInside)
        return button
    }

    /// Marks the candidate the marked text shows, and slides it into view.
    private func highlight(_ index: Int?) {
        shownHighlight = index
        for (i, view) in candidateStack.arrangedSubviews.enumerated() {
            guard let button = view as? UIButton else { continue }
            let on = i == index
            button.backgroundColor = on ? theme?.accent : nil
            button.setTitleColor(on ? theme?.onAccent : theme?.keyText, for: .normal)
            button.accessibilityTraits = on ? [.button, .selected] : .button
        }
        guard let index, candidateStack.arrangedSubviews.indices.contains(index) else { return }
        layoutIfNeeded()
        scroll.scrollRectToVisible(candidateStack.arrangedSubviews[index].frame.insetBy(dx: -24, dy: 0), animated: false)
    }
}

/// Every candidate at once, in the keys' place (the bar's expand arrow): rows of candidates
/// that scroll, and an arrow back to the keys.
final class CandidateGridView: UIView {
    var onChoose: (@MainActor (Int) -> Void)?
    var onClose: (@MainActor () -> Void)?

    private let scroll = UIScrollView()
    private let content = UIView()
    private let closeButton = UIButton(type: .system)
    private var buttons: [UIButton] = []
    private var theme: KeyboardTheme?

    private static let rowHeight: CGFloat = 46
    private static let padding: CGFloat = 14

    override init(frame: CGRect) {
        super.init(frame: frame)
        closeButton.setImage(UIImage(systemName: "chevron.up", withConfiguration: UIImage.SymbolConfiguration(pointSize: 16, weight: .semibold)), for: .normal)
        closeButton.accessibilityLabel = "back to the keys"
        closeButton.addAction(UIAction { [weak self] _ in self?.onClose?() }, for: .touchUpInside)
        scroll.alwaysBounceVertical = true
        scroll.addSubview(content)
        for view in [scroll, closeButton] {
            view.translatesAutoresizingMaskIntoConstraints = false
            addSubview(view)
        }
        NSLayoutConstraint.activate([
            closeButton.topAnchor.constraint(equalTo: topAnchor),
            closeButton.trailingAnchor.constraint(equalTo: trailingAnchor),
            closeButton.widthAnchor.constraint(equalToConstant: 44),
            closeButton.heightAnchor.constraint(equalToConstant: Self.rowHeight),
            scroll.topAnchor.constraint(equalTo: topAnchor),
            scroll.leadingAnchor.constraint(equalTo: leadingAnchor),
            scroll.trailingAnchor.constraint(equalTo: closeButton.leadingAnchor),
            scroll.bottomAnchor.constraint(equalTo: bottomAnchor),
        ])
    }

    required init?(coder: NSCoder) { fatalError("init(coder:) is not used") }

    func apply(_ theme: KeyboardTheme) {
        self.theme = theme
        backgroundColor = theme.background
        closeButton.tintColor = theme.keyText
    }

    func show(_ candidates: [String], highlighted: Int?) {
        buttons.forEach { $0.removeFromSuperview() }
        buttons = candidates.enumerated().map { index, text in
            let button = UIButton(type: .custom)
            button.setTitle(text, for: .normal)
            button.titleLabel?.font = .systemFont(ofSize: 18)
            let on = index == highlighted
            button.setTitleColor(on ? theme?.onAccent : theme?.keyText, for: .normal)
            button.setTitleColor(theme?.hintText, for: .highlighted)
            button.backgroundColor = on ? theme?.accent : theme?.key
            button.layer.cornerRadius = 6
            button.accessibilityLanguage = "ja"
            button.addAction(UIAction { [weak self] _ in self?.onChoose?(index) }, for: .touchUpInside)
            content.addSubview(button)
            return button
        }
        scroll.setContentOffset(.zero, animated: false)
        setNeedsLayout()
    }

    /// The candidates wrapped into rows, each as wide as its text.
    override func layoutSubviews() {
        super.layoutSubviews()
        let width = scroll.bounds.width
        guard width > 0 else { return }
        var x: CGFloat = 4, y: CGFloat = 4
        for button in buttons {
            let textWidth = button.titleLabel?.intrinsicContentSize.width ?? 0
            let w = min(width - 8, max(48, textWidth + 2 * Self.padding))
            if x + w > width - 4, x > 4 {
                x = 4
                y += Self.rowHeight
            }
            button.frame = CGRect(x: x, y: y, width: w, height: Self.rowHeight - 6)
            x += w + 6
        }
        let height = y + Self.rowHeight
        content.frame = CGRect(x: 0, y: 0, width: width, height: height)
        scroll.contentSize = CGSize(width: width, height: height)
    }
}
