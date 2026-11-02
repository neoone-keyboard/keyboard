/// What a key does when tapped. Typing logic depends only on this, never on UIKit.
public enum KeyAction: Hashable, Sendable {
    case character(String)
    case space
    case backspace
    case shift
    case returnKey
    case nextKeyboard
}
