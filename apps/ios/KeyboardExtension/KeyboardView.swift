import UIKit
import KeyboardCore

/// Draws the keys and turns touches into GestureEvents. Typing decisions live in
/// KeyGesture and InputController; this view only does layout and touch plumbing.
final class KeyboardView: UIView, UIInputViewAudioFeedback {
    var onEvent: (@MainActor (GestureEvent) -> Void)?
    var playsClicks = false
    var enableInputClicksWhenVisible: Bool { true }

    private var layout = KeyboardLayout(rows: [])
    private var theme: KeyboardTheme?
    private var label: (@MainActor (Key) -> String)?
    private var frames: [KeyFrame] = []
    private var caps: [KeyCapView] = []
    private var activeTouches: [ObjectIdentifier: (gesture: KeyGesture, index: Int)] = [:]
    private var timer: Timer?
    private let popup = AlternatesPopupView()

    override init(frame: CGRect) {
        super.init(frame: frame)
        isMultipleTouchEnabled = true
        clipsToBounds = false
        addSubview(popup)
    }

    required init?(coder: NSCoder) { fatalError("init(coder:) is not used") }

    /// Replaces every key. Cancels touches in progress, since their keys are gone.
    func configure(layout: KeyboardLayout, theme: KeyboardTheme, label: @escaping @MainActor (Key) -> String) {
        cancelAllTouches()
        self.layout = layout
        self.theme = theme
        self.label = label
        caps.forEach { $0.removeFromSuperview() }
        caps = layout.rows.flatMap { $0 }.map { _ in
            let cap = KeyCapView()
            insertSubview(cap, belowSubview: popup)
            return cap
        }
        refreshLabels()
        setNeedsLayout()
    }

    /// Re-reads every key's label, e.g. after shift changes.
    func refreshLabels() {
        guard let theme, let label else { return }
        for (cap, key) in zip(caps, layout.rows.flatMap { $0 }) {
            cap.configure(title: label(key), hint: key.alternates.first, special: Self.isSpecial(key.action), theme: theme)
        }
    }

    override func layoutSubviews() {
        super.layoutSubviews()
        frames = KeyboardGeometry.frames(for: layout, in: bounds.size)
        for (cap, frame) in zip(caps, frames) {
            cap.frame = frame.rect
        }
    }

    // MARK: Touches

    override func touchesBegan(_ touches: Set<UITouch>, with event: UIEvent?) {
        for touch in touches {
            let point = touch.location(in: self)
            guard let index = KeyboardGeometry.index(at: point, in: frames) else { continue }
            let frame = frames[index]
            let gesture = KeyGesture(key: frame.key, at: point, time: touch.timestamp, alternateWidth: frame.rect.width)
            activeTouches[ObjectIdentifier(touch)] = (gesture, index)
            caps[index].isPressed = true
            if playsClicks { UIDevice.current.playInputClick() }
        }
        startTimerIfNeeded()
    }

    override func touchesMoved(_ touches: Set<UITouch>, with event: UIEvent?) {
        for touch in touches {
            let id = ObjectIdentifier(touch)
            guard var entry = activeTouches[id] else { continue }
            let events = entry.gesture.moved(to: touch.location(in: self))
            activeTouches[id] = entry
            handle(events, for: id)
        }
    }

    override func touchesEnded(_ touches: Set<UITouch>, with event: UIEvent?) {
        for touch in touches {
            guard let entry = activeTouches.removeValue(forKey: ObjectIdentifier(touch)) else { continue }
            caps[entry.index].isPressed = false
            popup.hide()
            handle(entry.gesture.ended(), for: nil)
        }
        stopTimerIfIdle()
    }

    override func touchesCancelled(_ touches: Set<UITouch>, with event: UIEvent?) {
        for touch in touches {
            guard let entry = activeTouches.removeValue(forKey: ObjectIdentifier(touch)) else { continue }
            caps[entry.index].isPressed = false
        }
        popup.hide()
        stopTimerIfIdle()
    }

    private func handle(_ events: [GestureEvent], for id: ObjectIdentifier?) {
        for event in events {
            switch event {
            case .showAlternates(let options):
                guard let id, var entry = activeTouches[id], let theme else { continue }
                let x = popup.show(options, above: frames[entry.index].rect, within: bounds, theme: theme)
                entry.gesture.setAlternatesOrigin(x: x)
                activeTouches[id] = entry
            case .highlightAlternate(let index):
                popup.highlight(index)
            default:
                onEvent?(event)
            }
        }
    }

    // MARK: Timer (long-press and backspace repeat)

    private func startTimerIfNeeded() {
        guard timer == nil, !activeTouches.isEmpty else { return }
        timer = Timer.scheduledTimer(withTimeInterval: 0.02, repeats: true) { [weak self] _ in
            MainActor.assumeIsolated { self?.tick() }
        }
    }

    private func tick() {
        let now = ProcessInfo.processInfo.systemUptime
        for id in Array(activeTouches.keys) {
            guard var entry = activeTouches[id] else { continue }
            let events = entry.gesture.tick(at: now)
            activeTouches[id] = entry
            handle(events, for: id)
        }
    }

    private func stopTimerIfIdle() {
        guard activeTouches.isEmpty else { return }
        timer?.invalidate()
        timer = nil
    }

    private func cancelAllTouches() {
        activeTouches.removeAll()
        caps.forEach { $0.isPressed = false }
        popup.hide()
        stopTimerIfIdle()
    }

    private static func isSpecial(_ action: KeyAction) -> Bool {
        if case .character = action { return false }
        return action != .space
    }
}
