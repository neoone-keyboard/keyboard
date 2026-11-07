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
    #expect(settings.autocorrect, "autocorrect is on until the user turns it off")
    #expect(settings.emojiSuggestions, "emoji suggestions are on until the user turns them off")
    #expect(settings.suggestionCount == 10, "ten suggestions until the user picks another number")
}

@Test func roundTrips() {
    let defaults = freshDefaults()
    var settings = KeyboardSettings()
    settings.theme = .dark
    settings.accent = .teal
    settings.numberRow = true
    settings.keyClicks = true
    settings.autocorrect = false
    settings.emojiSuggestions = false
    settings.suggestionCount = 20
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

@Test func suggestionCountIsOneOfTheChoices() {
    #expect(KeyboardSettings.suggestionCounts == [3, 10, 20])
    for count in KeyboardSettings.suggestionCounts {
        let defaults = freshDefaults()
        var settings = KeyboardSettings()
        settings.suggestionCount = count
        settings.save(to: defaults)
        #expect(KeyboardSettings.load(from: defaults).suggestionCount == count)
    }
    let defaults = freshDefaults()
    defaults.set(7, forKey: "suggestionCount")
    #expect(KeyboardSettings.load(from: defaults).suggestionCount == 10)
}

@Test func appAccentIsTheIconsIndigo() {
    // tools/icon/render.swift lights the icon's 1 key in #5459CC.
    let rgb = AccentChoice.app.rgb
    #expect([rgb.red, rgb.green, rgb.blue].map { Int(($0 * 255).rounded()) } == [0x54, 0x59, 0xCC])
    #expect(AccentChoice.app.lightText)
}
