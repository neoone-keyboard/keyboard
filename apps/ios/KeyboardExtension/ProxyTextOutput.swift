import UIKit
import KeyboardCore

/// Sends KeyboardCore's text to the host app's text field.
@MainActor
final class ProxyTextOutput: TextOutput {
    private let proxy: @MainActor () -> UITextDocumentProxy

    init(_ proxy: @escaping @MainActor () -> UITextDocumentProxy) {
        self.proxy = proxy
    }

    func insertText(_ text: String) { proxy().insertText(text) }
    func deleteBackward() { proxy().deleteBackward() }
}
