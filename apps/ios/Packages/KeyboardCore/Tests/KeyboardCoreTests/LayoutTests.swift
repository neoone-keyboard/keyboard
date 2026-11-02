import Testing
import KeyboardCore

@Test func qwertyHasFourRowsWithExpectedKeyCounts() {
    #expect(KeyboardLayout.qwerty.rows.map(\.count) == [10, 9, 9, 3])
}

@Test func qwertyContainsEveryLetterOnce() {
    let letters = KeyboardLayout.qwerty.rows.flatMap { $0 }.compactMap { key -> String? in
        if case .character(let c) = key.action { return c }
        return nil
    }
    #expect(letters.sorted() == "abcdefghijklmnopqrstuvwxyz".map(String.init))
}
