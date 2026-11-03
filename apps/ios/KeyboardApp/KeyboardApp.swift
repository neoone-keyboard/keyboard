import SwiftUI

@main
struct KeyboardApp: App {
    init() {
        MetricsSubscriber.shared.start()
        #if DEBUG
        RuntimeCheck.runFromLaunchArguments()
        KeyboardCycleCheck.runFromLaunchArguments()
        #endif
    }

    var body: some Scene {
        WindowGroup { ContentView() }
    }
}

struct ContentView: View {
    @State private var testText = ""

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
                    Text("Settings are in the keyboard: touch and hold മ / EN.")
                }
                Section("Stability") {
                    LabeledContent("Saved MetricKit reports", value: "\(MetricsSubscriber.reportCount())")
                }
                Section {
                    NavigationLink("About and credits") { AboutView() }
                }
            }
            .navigationTitle("Malayalam Keyboard")
        }
    }
}
