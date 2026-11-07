import UIKit
import KeyboardCore

/// Draws the keys and turns touches into GestureEvents. Typing decisions live in
/// KeyGesture and InputController; this view only does layout and touch plumbing.
final class KeyboardView: UIView {
    var onEvent: (@MainActor (GestureEvent) -> Void)?
    /// A finger came down on a key, before its gesture is known (the key-up may never type it).
    var onPress: (@MainActor (KeyAction) -> Void)?
    /// Clicks only sound because the controller's input view is a KeyboardInputView.
    var playsClicks = false

    private var layout = KeyboardLayout(rows: [])
    private var theme: KeyboardTheme?
    private var label: (@MainActor (Key) -> String)?
    private var frames: [KeyFrame] = []
    private var caps: [KeyCapView] = []
    /// `index` is the touched key's cap, or nil once the keys were replaced mid-touch.
    private var activeTouches: [ObjectIdentifier: (gesture: KeyGesture, index: Int?)] = [:]
    private var timer: Timer?
    private let popup = AlternatesPopupView()
    /// The touch whose long-press is showing the alternates popup.
    private var popupOwner: ObjectIdentifier?
    /// The line under a swiping finger (`SwipeTrail`), over the keys.
    private let trail = CAShapeLayer()
    /// While a finger swipes, no other finger types.
    private var swiping: Bool { activeTouches.values.contains { $0.gesture.isSwiping } }

    /// The letter keys' caps, lowercase, in this view's points: what a swipe is decoded against.
    var letterKeys: [(letter: String, rect: CGRect)] {
        frames.compactMap { frame in
            guard case .character(let c) = frame.key.action, c.count == 1,
                  let scalar = c.unicodeScalars.first, scalar.isASCII, scalar.properties.isAlphabetic
            else { return nil }
            return (c.lowercased(), frame.rect)
        }
    }

    override init(frame: CGRect) {
        super.init(frame: frame)
        isMultipleTouchEnabled = true
        clipsToBounds = false
        addSubview(popup)
        trail.zPosition = 1
        layer.addSublayer(trail)
    }

    required init?(coder: NSCoder) { fatalError("init(coder:) is not used") }

    /// Replaces every key. Touches in progress keep their gesture (another finger's
    /// key still types when this one switches page) but lose their highlight and popup.
    func configure(layout: KeyboardLayout, theme: KeyboardTheme, label: @escaping @MainActor (Key) -> String) {
        for id in Array(activeTouches.keys) { activeTouches[id]?.index = nil }
        hidePopup()
        self.layout = layout
        self.theme = theme
        self.label = label
        trail.fillColor = theme.accent.cgColor
        caps.forEach { $0.removeFromSuperview() }
        caps = layout.rows.flatMap { $0 }.map { _ in
            let cap = KeyCapView()
            insertSubview(cap, belowSubview: popup)
            return cap
        }
        refreshLabels()
        layoutKeys()
        setNeedsLayout()
    }

    /// Drops every key, with its drawn labels, for a keyboard iOS keeps but no longer shows;
    /// `configure` makes them again.
    func removeKeys() {
        cancelAllTouches()
        hidePopup()
        caps.forEach { $0.removeFromSuperview() }
        caps = []
        frames = []
        layout = KeyboardLayout(rows: [])
    }

    /// Re-reads every key's label, e.g. after shift changes.
    func refreshLabels() {
        guard let theme, let label else { return }
        for (cap, key) in zip(caps, layout.rows.flatMap { $0 }) {
            let title = label(key)
            cap.configure(
                title: title, hint: Self.hint(key), special: Self.isSpecial(key.action),
                symbol: Self.symbol(key.action, label: title), theme: theme
            )
        }
    }

    /// An SF Symbol drawn in place of the label: the emoji key's smiley, and shift and
    /// backspace, whose text arrows draw too small (design-ref KeyboardLight.dc.html).
    private static func symbol(_ action: KeyAction, label: String) -> String? {
        switch action {
        case .emoji: "face.smiling"
        case .backspace: "delete.left"
        case .shift:
            switch label {
            case "⬆︎": "shift.fill"
            case "⇪": "capslock.fill"
            default: "shift"
            }
        default: nil
        }
    }

    override func layoutSubviews() {
        super.layoutSubviews()
        layoutKeys()
    }

    override func willMove(toWindow newWindow: UIWindow?) {
        super.willMove(toWindow: newWindow)
        if newWindow == nil { cancelAllTouches() }
    }

    /// Keeps `frames` and `caps` in step: both always describe the current layout.
    private func layoutKeys() {
        frames = KeyboardGeometry.frames(for: layout, in: bounds.size)
        for (cap, frame) in zip(caps, frames) {
            cap.frame = frame.rect
        }
    }

    // MARK: Touches

    override func touchesBegan(_ touches: Set<UITouch>, with event: UIEvent?) {
        guard !swiping else { return }
        for touch in touches {
            let point = touch.location(in: self)
            guard let index = KeyboardGeometry.index(at: point, in: frames), frames.indices.contains(index) else { continue }
            let frame = frames[index]
            let gesture = KeyGesture(
                key: frame.key, at: point, time: touch.timestamp,
                alternateWidth: frame.rect.width, alternateHeight: frame.rect.height
            )
            activeTouches[ObjectIdentifier(touch)] = (gesture, index)
            setPressed(true, at: index)
            onPress?(frame.key.action)
            if playsClicks { UIDevice.current.playInputClick() }
        }
        startTimerIfNeeded()
    }

    override func touchesMoved(_ touches: Set<UITouch>, with event: UIEvent?) {
        for touch in touches {
            let id = ObjectIdentifier(touch)
            guard var entry = activeTouches[id] else { continue }
            let wasSwiping = entry.gesture.isSwiping
            let events = entry.gesture.moved(to: touch.location(in: self))
            activeTouches[id] = entry
            if entry.gesture.isSwiping {
                if !wasSwiping { startSwipe(by: id) }
                drawTrail(entry.gesture.swipePoints)
            }
            handle(events, for: id)
        }
    }

    override func touchesEnded(_ touches: Set<UITouch>, with event: UIEvent?) {
        for touch in touches {
            let id = ObjectIdentifier(touch)
            guard let entry = activeTouches.removeValue(forKey: id) else { continue }
            setPressed(false, at: entry.index)
            if id == popupOwner { hidePopup() }
            if entry.gesture.isSwiping { fadeTrail() }
            handle(entry.gesture.ended(), for: nil)
        }
        stopTimerIfIdle()
    }

    override func touchesCancelled(_ touches: Set<UITouch>, with event: UIEvent?) {
        for touch in touches {
            let id = ObjectIdentifier(touch)
            guard let entry = activeTouches.removeValue(forKey: id) else { continue }
            setPressed(false, at: entry.index)
            if id == popupOwner { hidePopup() }
            if entry.gesture.isSwiping { fadeTrail() }
        }
        stopTimerIfIdle()
    }

    // MARK: Swipe trail

    /// A finger began to swipe: its key is no longer pressed, and any other finger is dropped
    /// without typing.
    private func startSwipe(by id: ObjectIdentifier) {
        for other in Array(activeTouches.keys) where other != id {
            setPressed(false, at: activeTouches[other]?.index)
            activeTouches.removeValue(forKey: other)
        }
        setPressed(false, at: activeTouches[id]?.index)
        trail.removeAllAnimations()
        trail.opacity = 1
    }

    private func drawTrail(_ points: [CGPoint]) {
        let outline = SwipeTrail.outline(points)
        guard let first = outline.first, let head = points.last else { return }
        let path = CGMutablePath()
        path.move(to: first)
        outline.dropFirst().forEach { path.addLine(to: $0) }
        path.closeSubpath()
        let r = SwipeTrail.headWidth / 2
        path.addEllipse(in: CGRect(x: head.x - r, y: head.y - r, width: 2 * r, height: 2 * r))
        CATransaction.begin()
        CATransaction.setDisableActions(true)
        trail.path = path
        CATransaction.commit()
    }

    /// The trail fades over 0.2 s once the finger lifts.
    private func fadeTrail() {
        CATransaction.begin()
        CATransaction.setAnimationDuration(0.2)
        CATransaction.setCompletionBlock { [weak self] in
            guard let self, trail.opacity == 0 else { return }
            trail.path = nil
        }
        trail.opacity = 0
        CATransaction.commit()
    }

    private func handle(_ events: [GestureEvent], for id: ObjectIdentifier?) {
        for event in events {
            switch event {
            case .showAlternates(let options):
                guard let id, var entry = activeTouches[id], let index = entry.index,
                      frames.indices.contains(index), let theme else { continue }
                // The key's own character (the full stop's popup holds it) sits right above it.
                let own: String? = if case .character(let c) = frames[index].key.action { c } else { nil }
                let anchor = options.firstIndex { $0 == own } ?? 0
                let x = popup.show(
                    options, columns: frames[index].key.alternatesGrid.columns, anchor: anchor,
                    above: frames[index].rect, within: bounds, theme: theme
                )
                popupOwner = id
                let highlight = entry.gesture.setAlternatesOrigin(x: x)
                activeTouches[id] = entry
                handle(highlight, for: id)
            case .highlightAlternate(let index):
                guard let id, id == popupOwner else { continue }
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

    /// Drops every touch in progress so none of them types when lifted.
    func cancelAllTouches() {
        activeTouches.removeAll()
        trail.path = nil
        caps.forEach { $0.isPressed = false }
        hidePopup()
        stopTimerIfIdle()
    }

    private func hidePopup() {
        popup.hide()
        popupOwner = nil
    }

    private func setPressed(_ pressed: Bool, at index: Int?) {
        guard let index, caps.indices.contains(index) else { return }
        caps[index].isPressed = pressed
    }

    /// The small hint on a key: its first alternate, or for a key whose popup holds the
    /// key itself (the full stop), the option to its right (`?`).
    private static func hint(_ key: Key) -> String? {
        if case .character(let own) = key.action, let index = key.alternates.firstIndex(of: own) {
            return key.alternates.indices.contains(index + 1) ? key.alternates[index + 1] : nil
        }
        return key.alternates.first
    }

    private static func isSpecial(_ action: KeyAction) -> Bool {
        if case .character = action { return false }
        return action != .space
    }
}
