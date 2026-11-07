import SwiftUI
import KeyboardCore

/// The app, its privacy in one sentence, and credits for every source of the word lists and
/// models and every library, with its licence (design-ref About.dc.html).
struct AboutView: View {
    /// The packs the keyboard loads, bundled and installed (`InstalledPacks.all`): their
    /// manifests credit their sources.
    private let packs = InstalledPacks.all()

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 20) {
                appCard
                HeaderedSection(title: "Word lists and models are built from") {
                    CreditList(credits: Credits.dataSources(packs: packs))
                }
                Card {
                    NavigationLink {
                        SoftwareView()
                    } label: {
                        HStack(spacing: 12) {
                            VStack(alignment: .leading, spacing: 2) {
                                Text("Open-source software").font(.system(size: 15))
                                Text("\(Credits.software.count) libraries in the keyboard's engine")
                                    .font(.system(size: 13))
                                    .foregroundStyle(.secondary)
                            }
                            Spacer(minLength: 8)
                            Chevron()
                        }
                        .padding(AppStyle.rowPadding)
                        .contentShape(Rectangle())
                    }
                    .buttonStyle(.plain)
                }
                #if DEBUG
                HeaderedSection(title: "Debug") {
                    Card {
                        NavigationLink {
                            RuntimeSpikeView()
                        } label: {
                            HStack {
                                Text("Runtime check").font(.system(size: 15))
                                Spacer()
                                Chevron()
                            }
                            .padding(AppStyle.rowPadding)
                            .contentShape(Rectangle())
                        }
                        .buttonStyle(.plain)
                    }
                }
                #endif
            }
            .padding(.horizontal, 16)
            .padding(.top, 8)
            .padding(.bottom, 32)
        }
        .background(Color(.systemGroupedBackground))
        .navigationTitle("About")
        .navigationBarTitleDisplayMode(.large)
    }

    private var appCard: some View {
        Card(padding: 16) {
            VStack(alignment: .leading, spacing: 10) {
                HStack(spacing: 12) {
                    AppLogo(size: 44)
                    VStack(alignment: .leading, spacing: 1) {
                        Text("NeoOne Keyboard").font(.system(size: 17, weight: .semibold))
                        Text("Version \(Self.version) · free and open source")
                            .font(.system(size: 13))
                            .foregroundStyle(.secondary)
                    }
                }
                Text("Suggestions, autocorrect, phonetic typing, swipe and emoji all run on your iPhone. Nothing you type leaves your phone.")
                    .font(.system(size: 14))
                    .foregroundStyle(.secondary)
                    .lineSpacing(2)
                    .fixedSize(horizontal: false, vertical: true)
            }
            .padding(.vertical, 2)
        }
    }

    /// "1.0 (1)", from the bundle.
    static var version: String {
        let info = Bundle.main.infoDictionary ?? [:]
        let short = info["CFBundleShortVersionString"] as? String ?? "?"
        let build = info["CFBundleVersion"] as? String
        return build.map { "\(short) (\($0))" } ?? short
    }
}

/// The engine's libraries, as a credit list.
private struct SoftwareView: View {
    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 8) {
                CreditList(credits: Credits.software)
                Text("Each library's page has its licence and source code.")
                    .font(.system(size: 13))
                    .foregroundStyle(.secondary)
                    .padding(.horizontal, 4)
            }
            .padding(.horizontal, 16)
            .padding(.top, 8)
            .padding(.bottom, 32)
        }
        .background(Color(.systemGroupedBackground))
        .navigationTitle("Open-source software")
        .navigationBarTitleDisplayMode(.inline)
    }
}

/// Credits as rows of a card: the name and a licence chip; each opens its attribution.
private struct CreditList: View {
    let credits: [Credit]

    var body: some View {
        Card {
            ForEach(Array(credits.enumerated()), id: \.element.id) { index, credit in
                if index > 0 { RowDivider() }
                NavigationLink {
                    CreditDetail(credit: credit)
                } label: {
                    HStack(spacing: 12) {
                        // Name and chip on one line when they fit, else the chip under the name.
                        ViewThatFits(in: .horizontal) {
                            HStack(spacing: 12) {
                                name(credit).fixedSize()
                                Spacer(minLength: 8)
                                LicenceChip(licence: credit.licence)
                            }
                            VStack(alignment: .leading, spacing: 6) {
                                name(credit).fixedSize(horizontal: false, vertical: true)
                                LicenceChip(licence: credit.licence)
                            }
                            .frame(maxWidth: .infinity, alignment: .leading)
                        }
                        Chevron()
                    }
                    .padding(EdgeInsets(top: 12, leading: 16, bottom: 12, trailing: 16))
                    .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                .accessibilityLabel("\(credit.name), \(credit.licence)")
            }
        }
    }
}

extension CreditList {
    private func name(_ credit: Credit) -> some View {
        Text(credit.name)
            .font(.system(size: 15))
            .multilineTextAlignment(.leading)
    }
}

private struct LicenceChip: View {
    let licence: String

    var body: some View {
        Text(licence)
            .font(.system(size: 12, weight: .medium))
            .foregroundStyle(.secondary)
            .multilineTextAlignment(.leading)
            .fixedSize(horizontal: false, vertical: true)
            .padding(.horizontal, 7)
            .padding(.vertical, 3)
            .background(Color(.tertiarySystemFill), in: RoundedRectangle(cornerRadius: 6, style: .continuous))
    }
}

/// One credit in full: its licence, with a link when it has one, and its attribution.
private struct CreditDetail: View {
    let credit: Credit

    var body: some View {
        ScrollView {
            Card(padding: 16) {
                VStack(alignment: .leading, spacing: 12) {
                    Text(credit.name).font(.system(size: 17, weight: .semibold))
                    HStack {
                        LicenceChip(licence: credit.licence)
                        Spacer()
                    }
                    Text(credit.attribution)
                        .font(.system(size: 14))
                        .foregroundStyle(.secondary)
                        .textSelection(.enabled)
                        .fixedSize(horizontal: false, vertical: true)
                    if let url = URL(string: credit.licenceURL) {
                        Link(destination: url) {
                            HStack(spacing: 4) {
                                Text(credit.licenceURL).font(.system(size: 14)).multilineTextAlignment(.leading)
                                Image(systemName: "arrow.up.right").font(.system(size: 12, weight: .semibold))
                            }
                        }
                    }
                }
            }
            .padding(.horizontal, 16)
            .padding(.top, 8)
        }
        .background(Color(.systemGroupedBackground))
        .navigationTitle(credit.name)
        .navigationBarTitleDisplayMode(.inline)
    }
}
