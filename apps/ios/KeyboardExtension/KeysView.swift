import UIKit
import KeyboardCore

/// Draws a KeyboardLayout as rows of buttons and reports taps as KeyActions.
final class KeysView: UIStackView {
    var onAction: (@MainActor (KeyAction) -> Void)?
    private var buttons: [(action: KeyAction, button: UIButton)] = []
    /// Always created; the view controller shows or hides it once the system has decided.
    private(set) var nextKeyboardButton: UIButton?

    init(layout: KeyboardLayout, configureNextKeyboard: (UIButton) -> Void) {
        super.init(frame: .zero)
        axis = .vertical
        distribution = .fillEqually
        spacing = 8
        for row in layout.rows {
            addArrangedSubview(makeRow(row, configureNextKeyboard: configureNextKeyboard))
        }
    }

    required init(coder: NSCoder) { fatalError("init(coder:) is not used") }

    func refreshLabels(_ label: @MainActor (KeyAction) -> String) {
        for (action, button) in buttons {
            button.setTitle(label(action), for: .normal)
        }
    }

    private func makeRow(_ keys: [Key], configureNextKeyboard: (UIButton) -> Void) -> UIStackView {
        let row = UIStackView()
        row.axis = .horizontal
        row.spacing = 6
        var reference: (button: UIButton, width: Double)?
        for key in keys {
            let button = makeButton(for: key.action)
            if key.action == .nextKeyboard {
                configureNextKeyboard(button)
                nextKeyboardButton = button
            } else {
                button.addAction(UIAction { [weak self] _ in self?.onAction?(key.action) }, for: .touchUpInside)
            }
            row.addArrangedSubview(button)
            if let reference {
                button.widthAnchor.constraint(
                    equalTo: reference.button.widthAnchor,
                    multiplier: key.width / reference.width
                ).isActive = true
            } else {
                reference = (button, key.width)
            }
            buttons.append((key.action, button))
        }
        return row
    }

    private func makeButton(for action: KeyAction) -> UIButton {
        let button = UIButton(type: .system)
        button.backgroundColor = .secondarySystemBackground
        button.layer.cornerRadius = 6
        button.setTitleColor(.label, for: .normal)
        if case .character = action {
            button.titleLabel?.font = .systemFont(ofSize: 22)
        } else {
            button.titleLabel?.font = .systemFont(ofSize: 16)
        }
        return button
    }
}
