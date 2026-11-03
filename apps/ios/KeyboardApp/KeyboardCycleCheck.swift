#if DEBUG
import UIKit

/// Opens and closes the keyboard over and over, typing a few words each time, so the
/// extension's memory can be watched across sessions (iOS keeps the extension process and
/// makes a new view controller per session). In the Simulator, make this keyboard the only
/// one before the first launch and reboot the device:
///
///     xcrun simctl spawn <udid> defaults write -g AppleKeyboards -array me.jishnu.keyboard.extension
///
/// then launch the app with `-keyboardCycles 40` (`xcrun simctl launch <udid> me.jishnu.keyboard
/// -keyboardCycles 40`). The extension's Library/Caches/diagnostics.log has a "Keyboard loaded
/// … footprint" and a "Keyboard released … footprint" line per session; `heap <pid>` on the
/// Mac counts what each session left behind. The text goes in through the field, as a host
/// app's own edit would, so the extension sees `textDidChange` and asks for suggestions.
@MainActor
enum KeyboardCycleCheck {
    static func runFromLaunchArguments() {
        let arguments = ProcessInfo.processInfo.arguments
        guard let flag = arguments.firstIndex(of: "-keyboardCycles"),
              arguments.indices.contains(flag + 1), let cycles = Int(arguments[flag + 1]) else { return }
        Task { await run(cycles: cycles) }
    }

    private static let words = ["hello ", "definitely ", "enthu ", "sukhamano ", "preferenc"]

    private static func run(cycles: Int) async {
        // The SwiftUI window appears a moment after launch.
        try? await Task.sleep(for: .seconds(2))
        guard let window = UIApplication.shared.connectedScenes
            .compactMap({ ($0 as? UIWindowScene)?.keyWindow }).first else { return }
        let field = UITextField(frame: CGRect(x: 20, y: 120, width: 300, height: 44))
        field.backgroundColor = .secondarySystemBackground
        window.addSubview(field)
        for _ in 0..<cycles {
            field.text = ""
            field.becomeFirstResponder()
            try? await Task.sleep(for: .seconds(1))
            for word in words {
                field.insertText(word)
                try? await Task.sleep(for: .milliseconds(150))
            }
            field.resignFirstResponder()
            try? await Task.sleep(for: .seconds(1))
        }
    }
}
#endif
