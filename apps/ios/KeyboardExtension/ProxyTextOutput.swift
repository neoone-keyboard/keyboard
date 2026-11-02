import UIKit
import KeyboardCore

/// Sends KeyboardCore's text to the host app's text field.
@MainActor
final class ProxyTextOutput: TextOutput {
    private let proxy: @MainActor () -> UITextDocumentProxy

    init(_ proxy: @escaping @MainActor () -> UITextDocumentProxy) {
        self.proxy = proxy
    }

    var contextBefore: String? { proxy().documentContextBeforeInput }
    var hasText: Bool { proxy().hasText }

    func insertText(_ text: String) { proxy().insertText(text) }
    func deleteBackward() { proxy().deleteBackward() }
    func adjustTextPosition(byCharacterOffset offset: Int) { proxy().adjustTextPosition(byCharacterOffset: offset) }
}
