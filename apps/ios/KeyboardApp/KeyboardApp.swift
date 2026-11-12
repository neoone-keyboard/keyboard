import SwiftUI
import UIKit
import KeyboardCore

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
    @Environment(\.scenePhase) private var scenePhase
    /// Set once the first-launch step is done (the app's own defaults; `-chooseLanguagesDone NO`
    /// as a launch argument shows it again).
    @AppStorage("chooseLanguagesDone") private var chooseLanguagesDone = false
    /// "Hint when there are more suggestions", in the App Group's defaults for the keyboard to
    /// read (`SharedSettings`); Android keeps the same key.
    @AppStorage(SharedSettings.suggestionMoreHintKey, store: UserDefaults(suiteName: PackLocations.appGroup))
    private var suggestionMoreHint = true
    /// The Languages screen, opened by `neoone://languages` (no longer used by the keyboard,
    /// whose settings panel now switches languages itself) or, in debug builds, by `-showLanguages` at launch, for screenshots.
    #if DEBUG
    @State private var showLanguages = ProcessInfo.processInfo.arguments.contains("-showLanguages")
    /// `-showAbout`: the About screen at launch, for screenshots.
    @State private var showAbout = ProcessInfo.processInfo.arguments.contains("-showAbout")
    /// `-showSettings`: the home screen scrolled to Settings at launch, for screenshots.
    private let scrollToSettings = ProcessInfo.processInfo.arguments.contains("-showSettings")
    #else
    @State private var showLanguages = false
    @State private var showAbout = false
    private let scrollToSettings = false
    #endif

    var body: some View {
        NavigationStack {
            ScrollViewReader { reader in
                ScrollView {
                    VStack(alignment: .leading, spacing: 20) {
                        header
                        TurnOnCard()
                        tryIt
                        languagesSection
                        settingsSection.id("settings")
                        #if DEBUG
                        // Diagnostics for development builds; a store build shows none.
                        stability
                        #endif
                    }
                    .padding(.horizontal, 16)
                    .padding(.top, 8)
                    .padding(.bottom, 24)
                }
                .onAppear { if scrollToSettings { reader.scrollTo("settings", anchor: .top) } }
            }
            .scrollDismissesKeyboard(.interactively)
            .background(Color(.systemGroupedBackground))
            // Without a navigation bar, the status bar needs its own backdrop over the scroll.
            .safeAreaInset(edge: .top, spacing: 0) {
                Color.clear.frame(height: 0).background(Color(.systemGroupedBackground))
            }
            .navigationTitle("NeoOne Keyboard")
            // The header below is the title; pushed screens keep their navigation bars.
            .toolbar(.hidden, for: .navigationBar)
            .navigationDestination(isPresented: $showLanguages) { LanguagesView(model: languages) }
            .navigationDestination(isPresented: $showAbout) { AboutView() }
            .onOpenURL { url in
                if url.scheme == "neoone", url.host() == "languages" { showLanguages = true }
            }
        }
        .tint(AppStyle.accent)
        // The keyboard's settings panel switches languages in the App Group: read them again
        // when the app comes back, and when the keyboard goes away inside the app (Try it),
        // where the app never stops being active.
        .onChange(of: scenePhase) { _, phase in
            if phase == .active { languages.reload() }
        }
        .onReceive(NotificationCenter.default.publisher(for: UIResponder.keyboardDidHideNotification)) { _ in
            languages.reload()
        }
        .sheet(isPresented: Binding(get: { !chooseLanguagesDone }, set: { _ in })) {
            FirstLaunchView(model: languages) { chooseLanguagesDone = true }
        }
        #if DEBUG
        .task { importFromLaunchArguments() }
        #endif
    }

    private var header: some View {
        HStack(spacing: 12) {
            AppLogo(size: 52)
            VStack(alignment: .leading, spacing: 2) {
                Text("NeoOne Keyboard")
                    .font(.system(size: 26, weight: .bold))
                    .accessibilityAddTraits(.isHeader)
                Text("The last keyboard you will ever need")
                    .font(.system(size: 14))
                    .foregroundStyle(.secondary)
            }
        }
        .padding(.horizontal, 4)
        .padding(.top, 8)
    }

    private var tryIt: some View {
        HeaderedSection(title: "Try it") {
            TextField("Type or swipe here", text: $testText, axis: .vertical)
                .font(.system(size: 16))
                .lineLimit(1...5)
                .padding(.horizontal, 14)
                .padding(.vertical, 13)
                .frame(minHeight: 48)
                .background(Color(.secondarySystemGroupedBackground),
                            in: RoundedRectangle(cornerRadius: 12, style: .continuous))
                .overlay(RoundedRectangle(cornerRadius: 12, style: .continuous)
                    .strokeBorder(Color(.separator), lineWidth: 1))
                .accessibilityIdentifier("try-it")
        }
    }

    /// The active languages first, then the others the keyboard has; each opens Languages.
    private var languagesSection: some View {
        HeaderedSection(title: "Languages") {
            Card {
                ForEach(languageRows) { row in
                    Button { showLanguages = true } label: {
                        HStack(spacing: 12) {
                            HStack(spacing: 6) {
                                Text(row.name)
                                if row.nativeName != row.name {
                                    Text(row.nativeName).font(.system(size: 16))
                                }
                            }
                            .font(.system(size: 15))
                            Spacer(minLength: 8)
                            Text(languages.isOn(row.id) ? "On" : "Off")
                                .font(.system(size: 13))
                                .foregroundStyle(languages.isOn(row.id) ? .secondary : .tertiary)
                        }
                        .padding(AppStyle.rowPadding)
                        .contentShape(Rectangle())
                    }
                    .buttonStyle(.plain)
                    RowDivider()
                }
                NavigationLink {
                    LanguagesView(model: languages)
                } label: {
                    HStack {
                        Text("Add or remove languages").font(.system(size: 15))
                        Spacer()
                        Chevron()
                    }
                    .padding(AppStyle.rowPadding)
                    .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                .accessibilityIdentifier("languages")
            }
        }
        .onAppear { languages.reload() }
    }

    private var settingsSection: some View {
        HeaderedSection(title: "Settings") {
            Card {
                Toggle(isOn: $suggestionMoreHint) {
                    VStack(alignment: .leading, spacing: 2) {
                        Text("Hint when there are more suggestions").font(.system(size: 15))
                        Text("A small mark at the edge of the suggestions when more are a swipe away.")
                            .font(.system(size: 13))
                            .foregroundStyle(.secondary)
                    }
                }
                .padding(AppStyle.rowPadding)
                .accessibilityIdentifier("suggestion-more-hint")
                RowDivider()
                keyboardRows
            }
        }
    }

    @ViewBuilder private var keyboardRows: some View {
        HStack(spacing: 12) {
            Text("Keyboard settings").font(.system(size: 15))
            Spacer(minLength: 8)
            Text("Hold \(Text(languageKeys).foregroundStyle(.primary))")
                .font(.system(size: 15))
                .foregroundStyle(.secondary)
        }
        .padding(AppStyle.rowPadding)
        .accessibilityElement(children: .combine)
        .accessibilityHint("Touch and hold the language key on the keyboard to open its settings.")
        RowDivider()
        NavigationLink {
            AboutView()
        } label: {
            HStack {
                Text("About and credits").font(.system(size: 15))
                Spacer()
                Chevron()
            }
            .padding(AppStyle.rowPadding)
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
    }

    /// How many MetricKit reports the app has saved (crash and hang diagnostics).
    private var stability: some View {
        HeaderedSection(title: "Stability") {
            Card {
                HStack {
                    Text("Saved MetricKit reports").font(.system(size: 15))
                    Spacer()
                    Text("\(MetricsSubscriber.reportCount())")
                        .font(.system(size: 15))
                        .foregroundStyle(.secondary)
                }
                .padding(AppStyle.rowPadding)
            }
        }
    }

    /// The active languages, then the rest, in the keyboard's order.
    private var languageRows: [LanguagesModel.Row] {
        languages.rows.filter { languages.isOn($0.id) } + languages.rows.filter { !languages.isOn($0.id) }
    }

    /// The language key's labels for the active languages: "മ / EN".
    private var languageKeys: String {
        let on = languages.rows.filter { languages.isOn($0.id) }
        // The packs' own keys first, English's EN last, as the key reads in the design.
        let keys = on.filter { $0.id != ActiveLanguages.english }.compactMap(\.languageKey)
            + on.filter { $0.id == ActiveLanguages.english }.compactMap(\.languageKey)
        return keys.isEmpty ? "the language key" : keys.joined(separator: " / ")
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
