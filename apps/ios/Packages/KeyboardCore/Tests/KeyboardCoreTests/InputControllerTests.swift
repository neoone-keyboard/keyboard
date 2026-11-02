import Testing
import KeyboardCore

@MainActor
final class RecordingOutput: TextOutput {
    var text = ""
    func insertText(_ text: String) { self.text += text }
    func deleteBackward() { if !text.isEmpty { text.removeLast() } }
}

@MainActor
struct InputControllerTests {
    let output = RecordingOutput()
    let input: InputController

    init() { input = InputController(output: output) }

    func type(_ actions: KeyAction..., startingAt time: Double = 0) {
        for (i, action) in actions.enumerated() {
            input.handle(action, at: time + Double(i) * 0.1)
        }
    }

    @Test func typesLowercaseLettersSpaceAndReturn() {
        type(.character("h"), .character("i"), .space, .returnKey)
        #expect(output.text == "hi \n")
    }

    @Test func backspaceDeletes() {
        type(.character("a"), .character("b"), .backspace)
        #expect(output.text == "a")
    }

    @Test func shiftCapitalisesOnlyTheNextLetter() {
        type(.shift, .character("h"), .character("i"))
        #expect(output.text == "Hi")
        #expect(input.shift == .off)
    }

    @Test func doubleTapShiftLocksCaps() {
        input.handle(.shift, at: 0)
        input.handle(.shift, at: 0.2)
        type(.character("o"), .character("k"), startingAt: 1)
        #expect(output.text == "OK")
        #expect(input.shift == .locked)
    }

    @Test func labelsFollowShiftState() {
        #expect(input.label(for: .character("a")) == "a")
        input.handle(.shift, at: 0)
        #expect(input.label(for: .character("a")) == "A")
        #expect(input.label(for: .shift) == "⇧")
        input.handle(.shift, at: 0.1)
        #expect(input.label(for: .shift) == "⇪")
    }
}
