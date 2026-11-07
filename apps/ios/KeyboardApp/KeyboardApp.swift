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
    @State private var languages = LanguagesModel()
    /// Set once the first-launch step is done (the app's own defaults; `-chooseLanguagesDone NO`
    /// as a launch argument shows it again).
    @AppStorage("chooseLanguagesDone") private var chooseLanguagesDone = false
    #if DEBUG
    /// `-showLanguages`: opens the Languages screen at launch, for screenshots.
    @State private var showLanguages = ProcessInfo.processInfo.arguments.contains("-showLanguages")
    #endif

    var body: some View {
        NavigationStack {
            Form {
                Section("Turn on the keyboard") {
                    Text("Settings → General → Keyboard → Keyboards → Add New Keyboard → NeoOne Keyboard")
                    Text("No Full Access needed. Nothing you type leaves your phone.")
                        .foregroundStyle(.secondary)
                }
                Section("Try it") {
                    TextField("Type here", text: $testText, axis: .vertical)
                }
                Section {
                    NavigationLink {
                        LanguagesView(model: languages)
                    } label: {
                        LabeledContent("Languages", value: activeNames)
                    }
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
            .navigationTitle("NeoOne Keyboard")
            #if DEBUG
            .navigationDestination(isPresented: $showLanguages) { LanguagesView(model: languages) }
            #endif
        }
        .sheet(isPresented: Binding(get: { !chooseLanguagesDone }, set: { _ in })) {
            FirstLaunchView(model: languages) { chooseLanguagesDone = true }
        }
        #if DEBUG
        .task { importFromLaunchArguments() }
        #endif
    }

    /// The active languages' names, in their own scripts: "English, മലയാളം".
    private var activeNames: String {
        languages.rows.filter { languages.isOn($0.id) }.map(\.nativeName).joined(separator: ", ")
    }

    #if DEBUG
    /// `-importPack <path>`: imports that file as the document picker would, for screenshots of
    /// the import and its refusals.
    private func importFromLaunchArguments() {
        let arguments = ProcessInfo.processInfo.arguments
        guard let flag = arguments.firstIndex(of: "-importPack"), arguments.indices.contains(flag + 1) else { return }
        languages.importPack(from: URL(fileURLWithPath: arguments[flag + 1]))
    }
    #endif
}
