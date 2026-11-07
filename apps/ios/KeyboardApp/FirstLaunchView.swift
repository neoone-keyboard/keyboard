import SwiftUI
import KeyboardCore

/// The first-launch step: which languages to turn on (M6 spec, "First launch"). Shown once,
/// over the main screen; the choice it stores is what the keyboard uses when it next starts.
struct FirstLaunchView: View {
    @Bindable var model: LanguagesModel
    let done: () -> Void

    var body: some View {
        NavigationStack {
            Form {
                Section {
                    Text("Choose the languages you type. The keyboard suggests words in these, and its language key switches between them.")
                }
                Section {
                    ForEach(model.rows) { row in
                        Toggle(isOn: Binding(get: { model.isOn(row.id) }, set: { _ in model.toggle(row.id) })) {
                            VStack(alignment: .leading, spacing: 4) {
                                Text(row.nativeName).font(.headline)
                                if row.nativeName != row.name {
                                    Text(row.name).font(.subheadline).foregroundStyle(.secondary)
                                }
                            }
                            .padding(.vertical, 2)
                        }
                        .disabled(!model.canToggle(row.id))
                    }
                } header: {
                    Text("Your languages")
                } footer: {
                    Text("Up to \(ActiveLanguages.maxActive) at a time. You can change this, or add more languages, in Languages.")
                }
                Section {
                    Button("Continue") {
                        model.store()
                        done()
                    }
                }
            }
            .navigationTitle("Welcome")
        }
        .interactiveDismissDisabled()
    }
}
