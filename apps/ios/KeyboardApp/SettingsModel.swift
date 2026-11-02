import Foundation
import Observation
import KeyboardCore

/// Reads and writes the keyboard's settings in the shared App Group.
@MainActor
@Observable
final class SettingsModel {
    private let defaults = KeyboardSettings.sharedDefaults ?? .standard

    var settings: KeyboardSettings {
        didSet { settings.save(to: defaults) }
    }

    init() {
        settings = KeyboardSettings.load(from: defaults)
    }
}
