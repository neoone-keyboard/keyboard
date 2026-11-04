import SwiftUI
import KeyboardCore

/// Credits every source of the word lists and models, and every library, with its licence.
struct AboutView: View {
    var body: some View {
        List {
            Section {
                Text("Suggestions, autocorrect and Manglish run entirely on your iPhone from word lists and models built from, and modified from, the sources below. Nothing you type leaves your phone.")
            }
            Section("Sources") {
                ForEach(Credits.dataSources) { CreditRow(credit: $0) }
            }
            Section("Software") {
                ForEach(Credits.software) { CreditRow(credit: $0) }
            }
            #if DEBUG
            Section("Debug") {
                NavigationLink("Runtime check") { RuntimeSpikeView() }
            }
            #endif
        }
        .navigationTitle("About")
    }
}

private struct CreditRow: View {
    let credit: Credit

    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            Text(credit.name).font(.headline)
            if let url = URL(string: credit.licenceURL) {
                Link(credit.licence, destination: url).font(.subheadline)
            } else {
                Text(credit.licence).font(.subheadline)
            }
            Text(credit.attribution).font(.footnote).foregroundStyle(.secondary)
        }
        .padding(.vertical, 2)
    }
}
