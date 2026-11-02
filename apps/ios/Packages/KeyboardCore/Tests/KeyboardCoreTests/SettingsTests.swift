import Testing
import Foundation
import KeyboardCore

private func freshDefaults() -> UserDefaults {
    let name = "test-\(UUID().uuidString)"
    let defaults = UserDefaults(suiteName: name)!
    defaults.removePersistentDomain(forName: name)
    return defaults
}

@Test func defaultsWhenNothingSaved() {
    let settings = KeyboardSettings.load(from: freshDefaults())
    #expect(settings == KeyboardSettings())
    #expect(settings.theme == .system)
    #expect(settings.accent == .saffron)
    #expect(!settings.numberRow)
    #expect(!settings.keyClicks)
}

@Test func roundTrips() {
    let defaults = freshDefaults()
    var settings = KeyboardSettings()
    settings.theme = .dark
    settings.accent = .teal
    settings.numberRow = true
    settings.keyClicks = true
    settings.save(to: defaults)
    #expect(KeyboardSettings.load(from: defaults) == settings)
}

@Test func unknownStoredValuesFallBackToDefaults() {
    let defaults = freshDefaults()
    defaults.set("neon", forKey: "theme")
    defaults.set("plaid", forKey: "accent")
    let settings = KeyboardSettings.load(from: defaults)
    #expect(settings.theme == .system)
    #expect(settings.accent == .saffron)
}
