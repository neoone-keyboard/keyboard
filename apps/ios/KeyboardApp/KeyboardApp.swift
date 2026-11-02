import SwiftUI
import KeyboardCore

@main
struct KeyboardApp: App {
    init() {
        MetricsSubscriber.shared.start()
    }

    var body: some Scene {
        WindowGroup { ContentView() }
    }
}

struct ContentView: View {
    @State private var testText = ""
    @State private var model = SettingsModel()

    var body: some View {
        NavigationStack {
            Form {
                Section("Turn on the keyboard") {
                    Text("Settings → General → Keyboard → Keyboards → Add New Keyboard → Malayalam Keyboard")
                    Text("No Full Access needed. Nothing you type leaves your phone.")
                        .foregroundStyle(.secondary)
                }
                Section("Try it") {
                    TextField("Type here", text: $testText, axis: .vertical)
                }
                Section("Keyboard") {
                    Picker("Theme", selection: $model.settings.theme) {
                        Text("Match system").tag(ThemeChoice.system)
                        Text("Light").tag(ThemeChoice.light)
                        Text("Dark").tag(ThemeChoice.dark)
                    }
                    Picker("Accent", selection: $model.settings.accent) {
                        Text("Saffron").tag(AccentChoice.saffron)
                        Text("Teal").tag(AccentChoice.teal)
                        Text("Indigo").tag(AccentChoice.indigo)
                        Text("Rose").tag(AccentChoice.rose)
                    }
                    Toggle("Number row", isOn: $model.settings.numberRow)
                    Toggle("Key click sound", isOn: $model.settings.keyClicks)
                }
                Section("Stability") {
                    LabeledContent("Saved MetricKit reports", value: "\(MetricsSubscriber.reportCount())")
                }
            }
            .navigationTitle("Malayalam Keyboard")
        }
    }
}
