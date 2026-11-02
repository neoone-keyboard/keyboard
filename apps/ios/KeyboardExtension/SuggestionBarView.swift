import UIKit

/// Three suggestion slots with the best one in the middle.
final class SuggestionBarView: UIStackView {
    private let slots = (0..<3).map { _ in UILabel() }

    override init(frame: CGRect) {
        super.init(frame: frame)
        axis = .horizontal
        distribution = .fillEqually
        for (index, label) in slots.enumerated() {
            label.textAlignment = .center
            label.font = .systemFont(ofSize: 17, weight: index == 1 ? .semibold : .regular)
            label.adjustsFontSizeToFitWidth = true
            label.minimumScaleFactor = 0.6
            addArrangedSubview(label)
        }
    }

    required init(coder: NSCoder) { fatalError("init(coder:) is not used") }

    /// `suggestions[0]` is the best and goes in the middle slot.
    func show(_ suggestions: [String]) {
        let order = [1, 0, 2]
        for (slot, index) in zip(slots, order) {
            slot.text = index < suggestions.count ? suggestions[index] : nil
        }
    }
}
