import UIKit

/// The controller's input view. `playInputClick()` only sounds when the input view
/// itself adopts `UIInputViewAudioFeedback`; a subview adopting it doesn't count.
final class KeyboardInputView: UIInputView, UIInputViewAudioFeedback {
    var enableInputClicksWhenVisible: Bool { true }
}
