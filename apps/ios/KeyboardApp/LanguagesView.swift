import SwiftUI
import KeyboardCore

/// The languages the keyboard has, which are on, and adding or removing them (M6 spec, "App: a
/// Languages screen"). The download section shows only when the app has a pack index.
struct LanguagesView: View {
    @Bindable var model: LanguagesModel
    @State private var picking = false
    @State private var removing: LanguagesModel.Row?
    @Environment(\.scenePhase) private var scenePhase

    var body: some View {
        List {
            Section {
                ForEach(model.rows) { row in
                    LanguageToggle(model: model, row: row)
                        .swipeActions(edge: .trailing) {
                            if row.removable {
                                Button("Remove", role: .destructive) { removing = row }
                            }
                        }
                }
            } header: {
                Text("Your languages")
            } footer: {
                Text(footer)
            }
            if !model.offers.isEmpty {
                Section("Updates") {
                    ForEach(model.offers, id: \.id) { offer in
                        if let entry = model.entry(for: offer) {
                            DownloadRow(model: model, entry: entry, action: "Update",
                                        detail: "Version \(offer.offered.version) · \(PackSize.text(entry.size))")
                        }
                    }
                }
            }
            Section {
                Button {
                    picking = true
                } label: {
                    HStack {
                        Text("Import from Files")
                        if model.importing {
                            Spacer()
                            ProgressView()
                        }
                    }
                }
                .disabled(model.importing)
            } header: {
                Text("Add a language")
            } footer: {
                Text("A language pack is a .neopack file. Save it in Files, then import it here.")
            }
            if model.indexURL != nil {
                downloadSection
            }
        }
        .navigationTitle("Languages")
        // The keyboard's settings panel may have changed the choice meanwhile (when it could
        // write the App Group).
        .onAppear { model.reload() }
        .onChange(of: scenePhase) { _, phase in
            if phase == .active { model.reload() }
        }
        .fileImporter(isPresented: $picking, allowedContentTypes: [PackImport.contentType, .zip]) { result in
            if case .success(let url) = result {
                model.importPack(from: url)
            }
        }
        .confirmationDialog(
            removing.map { "Remove \($0.name)?" } ?? "",
            isPresented: Binding(get: { removing != nil }, set: { if !$0 { removing = nil } }),
            titleVisibility: .visible,
            presenting: removing
        ) { row in
            Button("Remove", role: .destructive) { model.remove(row.id) }
        } message: { row in
            Text(row.builtIn
                 ? "The copy you added is removed and the keyboard goes back to the one built into the app."
                 : "The keyboard stops offering \(row.name). You can add it again later.")
        }
        .alert(item: $model.message) { message in
            Alert(title: Text(message.title), message: Text(message.text))
        }
        .task { await model.loadIndex() }
    }

    private var footer: String {
        var text = "Up to \(ActiveLanguages.maxActive) at a time. The language key on the keyboard switches between them. Changes apply when the keyboard next starts."
        if model.rows.count == 1 {
            text = "Only English so far. Import a language pack to add another. " + text
        }
        if model.rows.contains(where: \.removable) {
            text += " Swipe left on a language you added to remove it."
        }
        return text
    }

    @ViewBuilder private var downloadSection: some View {
        Section("Download") {
            switch model.index {
            case .idle, .loading:
                HStack {
                    Text("Looking for languages…").foregroundStyle(.secondary)
                    Spacer()
                    ProgressView()
                }
            case .failed(let text):
                Text(text).foregroundStyle(.secondary)
            case .loaded:
                if model.downloadable.isEmpty {
                    Text("You have every language there is to download.").foregroundStyle(.secondary)
                }
                ForEach(model.downloadable) { entry in
                    DownloadRow(model: model, entry: entry, action: "Get",
                                detail: [entry.nativeName == nil ? nil : entry.name, PackSize.text(entry.size)]
                                    .compactMap { $0 }.joined(separator: " · "))
                }
            }
        }
    }
}

/// One language: its name in its own script and in English, version and size, and whether it is on.
private struct LanguageToggle: View {
    let model: LanguagesModel
    let row: LanguagesModel.Row

    var body: some View {
        Toggle(isOn: Binding(get: { model.isOn(row.id) }, set: { _ in model.toggle(row.id) })) {
            VStack(alignment: .leading, spacing: 4) {
                Text(row.nativeName).font(.headline)
                Text(detail).font(.subheadline).foregroundStyle(.secondary)
                if let offer = model.offer(for: row.id) {
                    Text("Update available: \(offer.offered.version)").font(.footnote).foregroundStyle(.tint)
                }
            }
            .padding(.vertical, 2)
        }
        .disabled(!model.canToggle(row.id))
    }

    private var detail: String {
        var parts: [String] = []
        if row.nativeName != row.name { parts.append(row.name) }
        if let version = row.version { parts.append(version) }
        parts.append(PackSize.text(row.size))
        if row.builtIn { parts.append("Built in") }
        return parts.joined(separator: " · ")
    }
}

/// A pack the index offers, with its Get or Update button.
private struct DownloadRow: View {
    let model: LanguagesModel
    let entry: PackIndex.Entry
    let action: String
    let detail: String

    var body: some View {
        HStack {
            VStack(alignment: .leading, spacing: 4) {
                Text(entry.nativeName ?? entry.name).font(.headline)
                Text(detail).font(.subheadline).foregroundStyle(.secondary)
            }
            .padding(.vertical, 2)
            Spacer()
            if model.downloading.contains(entry.id) {
                ProgressView()
            } else {
                Button(action) { model.download(entry) }
                    .buttonStyle(.borderless)
                    .disabled(!model.downloading.isEmpty)
            }
        }
    }
}
