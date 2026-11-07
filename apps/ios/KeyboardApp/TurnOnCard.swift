import SwiftUI
import UIKit

/// "Turn it on": the three steps to add the keyboard, a button to the app's page in Settings
/// (where Keyboards is), and the privacy line.
struct TurnOnCard: View {
    @Environment(\.openURL) private var openURL

    var body: some View {
        Card(padding: 16) {
            VStack(alignment: .leading, spacing: 14) {
                SectionHeader("Turn it on")
                VStack(alignment: .leading, spacing: 12) {
                    Step(number: 1, text: Text("Open **Settings → General → Keyboard → Keyboards**"))
                    Step(number: 2, text: Text("Tap **Add New Keyboard**, then **NeoOne Keyboard**"))
                    Step(number: 3, text: Text("In any app, hold \(Image(systemName: "globe")) and pick it"))
                }
                Button {
                    if let url = URL(string: UIApplication.openSettingsURLString) { openURL(url) }
                } label: {
                    Text("Open Settings")
                        .font(.system(size: 16, weight: .semibold))
                        .foregroundStyle(AppStyle.onAccent)
                        .frame(maxWidth: .infinity, minHeight: 48)
                        .background(AppStyle.accent, in: RoundedRectangle(cornerRadius: 12, style: .continuous))
                }
                .buttonStyle(.plain)
                .accessibilityIdentifier("open-settings")
                HStack(spacing: 10) {
                    Image(systemName: "lock")
                        .font(.system(size: 15, weight: .medium))
                        .foregroundStyle(AppStyle.lock)
                        .accessibilityHidden(true)
                    Text("No Full Access needed. Nothing you type leaves your phone.")
                        .font(.system(size: 13))
                        .foregroundStyle(.secondary)
                        .fixedSize(horizontal: false, vertical: true)
                }
                .padding(.top, 2)
            }
        }
    }
}

private struct Step: View {
    let number: Int
    let text: Text

    var body: some View {
        HStack(alignment: .top, spacing: 12) {
            Text("\(number)")
                .font(.system(size: 13, weight: .semibold))
                .foregroundStyle(Color(.systemBackground))
                .frame(width: 24, height: 24)
                .background(Color(.label), in: Circle())
                .accessibilityHidden(true)
            text
                .font(.system(size: 15))
                .lineSpacing(3)
                .padding(.top, 2)
                .fixedSize(horizontal: false, vertical: true)
        }
        .accessibilityElement(children: .combine)
        .accessibilityLabel(Text("Step \(number): \(text)"))
    }
}
