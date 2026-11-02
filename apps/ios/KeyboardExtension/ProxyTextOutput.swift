import UIKit
import KeyboardCore

/// Sends KeyboardCore's text to the host app's text field, and mirrors each
/// edit in `edits` so a host that reports its text late isn't mistaken for
/// the user changing it.
@MainActor
final class ProxyTextOutput: TextOutput {
    private let proxy: @MainActor () -> UITextDocumentProxy
    private(set) var edits = OwnEditTracker()

    init(_ proxy: @escaping @MainActor () -> UITextDocumentProxy) {
        self.proxy = proxy
    }

    var contextBefore: String? { proxy().documentContextBeforeInput }
    var hasText: Bool { proxy().hasText }

    func insertText(_ text: String) {
        edits.inserted(text)
        proxy().insertText(text)
    }

    func deleteBackward() {
        edits.deletedBackward()
        proxy().deleteBackward()
    }

    func adjustTextPosition(byCharacterOffset offset: Int) {
        edits.movedCursor()
        proxy().adjustTextPosition(byCharacterOffset: offset)
    }

    func beginEdit() { edits.beginEdit(current: contextBefore) }
    func endEdit() { edits.endEdit(current: contextBefore) }
    /// The host or the user changed the text; what it says now is the new start.
    func resetEdits() { edits.reset(current: contextBefore) }
    /// Whether the host's text is what this keyboard's own edits produced.
    var hostTextIsOwn: Bool { edits.isOwn(contextBefore) }
}
