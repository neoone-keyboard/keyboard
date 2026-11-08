import SwiftUI
import KeyboardCore

/// The first-launch step: which languages to turn on, and downloads of the others (M6 spec,
/// "First launch"). Shown once, over the main screen; the choice it stores is what the keyboard
/// uses when it next starts. A release build bundles no pack, so until one is downloaded or
/// imported the list is English alone, and the keyboard types English.
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
                    Text("Up to \(ActiveLanguages.maxActive) at a time. You can change this, or import a language from Files, in Languages.")
                }
                if model.indexURL != nil {
                    PackDownloadSection(model: model)
                }
                Section {
                    Button("Continue") {
                        model.store()
                        done()
                    }
                }
            }
            .navigationTitle("Welcome")
            .alert(item: $model.message) { message in
                Alert(title: Text(message.title), message: Text(message.text))
            }
            .task { await model.loadIndex() }
        }
        .interactiveDismissDisabled()
    }
}
