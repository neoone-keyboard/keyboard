import UIKit
import KeyboardCore

/// The emoji panel, over the keys at their height: a search field (it opens `EmojiSearchView`),
/// the recents and Unicode's groups in one grid that scrolls down, and along the bottom ABC
/// (back to the letters), a symbol per group
/// that jumps the grid to it, and backspace (repeats while held). Holding an emoji that takes
/// skin tones shows them in the alternates popup; sliding picks one. The grid's cells are
/// reused and show pictures from `EmojiRenderer`, drawn off the main thread, not text: text
/// would keep every emoji ever shown in the system font's cache; a faint dot until a picture
/// is drawn, those on screen first. Made when opened and dropped on close, with the catalog
/// and the pictures; the renderer is made as soon as the panel is in the window, so its first
/// font is ready by the time the grid asks, and closed as it leaves.
final class EmojiPanelView: UIView, UICollectionViewDataSource, UICollectionViewDataSourcePrefetching,
    UICollectionViewDelegate {
    /// An emoji was picked: `emoji` as inserted, and its `base` when a tone was chosen for it.
    var onEmoji: (@MainActor (_ emoji: String, _ toneFor: String?) -> Void)?
    var onBackspace: (@MainActor () -> Void)?
    var onLetters: (@MainActor () -> Void)?
    var onSearch: (@MainActor () -> Void)?

    private struct Section {
        let name: String
        let symbol: String
        let items: [EmojiItem]
    }

    private var sections: [Section] = []
    private(set) var catalog: EmojiCatalog?
    private var renderer: EmojiRenderer?
    /// Cells filled since the panel opened, for the diagnostics log.
    private var cellsShown = 0
    /// For the log: when the panel was made, and the emoji of its first screen not drawn yet.
    private let madeAt = ContinuousClock.now
    private var firstScreen: Set<String>?
    private let log = Diagnostics(category: "emoji")
    /// For each emoji's tone; the recents shown are those of when the panel opened.
    private var history = EmojiHistory()
    private var theme: KeyboardTheme?
    private let layout = UICollectionViewFlowLayout()
    private lazy var grid = UICollectionView(frame: .zero, collectionViewLayout: layout)
    private let strip = UIStackView()
    /// Looks like a search field; the search itself opens in the top bar's place.
    private let searchField = UIControl()
    private let searchIcon = UIImageView(image: UIImage(systemName: "magnifyingglass"))
    private let searchLabel = UILabel()
    private let lettersButton = UIButton(type: .custom)
    private let backspaceButton = UIButton(type: .custom)
    private var categoryButtons: [UIButton] = []
    private var selectedSection = 0
    private let popup = AlternatesPopupView()
    /// The tones offered by the popup while it shows, and the one lit.
    private var popupOptions: [String] = []
    private var popupIndex = 0
    private var popupCellWidth: CGFloat = 0
    private var backspaceRepeat: Timer?

    /// How big an emoji is drawn, in points: 96 pixels at 3x, the size of one of the emoji
    /// font's own pictures, so it is drawn as it is, not scaled.
    static let emojiSize: CGFloat = 32
    /// A footprint line in the log every so many cells filled.
    private static let logEvery = 300
    private static let columns: CGFloat = 8
    private static let rowHeight: CGFloat = 44
    private static let stripHeight: CGFloat = 40
    private static let searchHeight: CGFloat = 34
    private static let headerHeight: CGFloat = 24
    private static let cellID = "emoji"
    private static let headerID = "header"
    /// SF Symbols for Unicode's groups, in emoji.txt's names; Recents first.
    private static let recentsSymbol = "clock"
    private static let symbols: [String: String] = [
        "Smileys & Emotion": "face.smiling",
        "People & Body": "hand.wave",
        "Animals & Nature": "pawprint",
        "Food & Drink": "fork.knife",
        "Travel & Places": "car",
        "Activities": "soccerball",
        "Objects": "lightbulb",
        "Symbols": "heart",
        "Flags": "flag",
    ]

    override init(frame: CGRect) {
        super.init(frame: frame)
        layout.minimumInteritemSpacing = 0
        layout.minimumLineSpacing = 0
        layout.sectionHeadersPinToVisibleBounds = false
        grid.dataSource = self
        grid.delegate = self
        grid.backgroundColor = .clear
        grid.showsVerticalScrollIndicator = false
        // Starts drawing the next row's emoji before it scrolls in.
        grid.prefetchDataSource = self
        grid.register(EmojiCell.self, forCellWithReuseIdentifier: Self.cellID)
        grid.register(EmojiHeader.self, forSupplementaryViewOfKind: UICollectionView.elementKindSectionHeader,
                      withReuseIdentifier: Self.headerID)
        // iOS 26 blurs scroll content near each edge, which smears the emoji by the strip.
        for edge in [grid.topEdgeEffect, grid.bottomEdgeEffect, grid.leftEdgeEffect, grid.rightEdgeEffect] {
            edge.isHidden = true
        }
        let hold = UILongPressGestureRecognizer(target: self, action: #selector(held(_:)))
        hold.minimumPressDuration = 0.35
        grid.addGestureRecognizer(hold)

        lettersButton.titleLabel?.font = .systemFont(ofSize: 15, weight: .medium)
        lettersButton.layer.cornerRadius = 6
        lettersButton.addAction(UIAction { [weak self] _ in self?.onLetters?() }, for: .touchUpInside)
        backspaceButton.setImage(UIImage(systemName: "delete.left", withConfiguration: Self.symbolConfig), for: .normal)
        backspaceButton.accessibilityLabel = "Delete"
        backspaceButton.layer.cornerRadius = 6
        backspaceButton.addAction(UIAction { [weak self] _ in self?.startBackspace() }, for: .touchDown)
        for event: UIControl.Event in [.touchUpInside, .touchUpOutside, .touchCancel] {
            backspaceButton.addAction(UIAction { [weak self] _ in self?.stopBackspace() }, for: event)
        }
        strip.axis = .horizontal
        strip.alignment = .fill
        strip.spacing = 2

        searchField.layer.cornerRadius = 8
        searchField.isAccessibilityElement = true
        searchField.accessibilityLabel = "Search emoji"
        searchField.accessibilityTraits = .searchField
        searchField.addAction(UIAction { [weak self] _ in self?.onSearch?() }, for: .touchUpInside)
        searchIcon.contentMode = .scaleAspectFit
        searchLabel.text = "Search emoji"
        searchLabel.font = .systemFont(ofSize: 16)
        for view in [searchIcon, searchLabel] as [UIView] {
            view.isUserInteractionEnabled = false
            view.translatesAutoresizingMaskIntoConstraints = false
            searchField.addSubview(view)
        }

        for view in [searchField, grid, strip] as [UIView] {
            view.translatesAutoresizingMaskIntoConstraints = false
            addSubview(view)
        }
        addSubview(popup)
        NSLayoutConstraint.activate([
            grid.leadingAnchor.constraint(equalTo: leadingAnchor),
            grid.trailingAnchor.constraint(equalTo: trailingAnchor),
            searchField.leadingAnchor.constraint(equalTo: leadingAnchor, constant: 8),
            searchField.trailingAnchor.constraint(equalTo: trailingAnchor, constant: -8),
            searchField.topAnchor.constraint(equalTo: topAnchor, constant: 6),
            searchField.heightAnchor.constraint(equalToConstant: Self.searchHeight),
            searchIcon.leadingAnchor.constraint(equalTo: searchField.leadingAnchor, constant: 8),
            searchIcon.centerYAnchor.constraint(equalTo: searchField.centerYAnchor),
            searchLabel.leadingAnchor.constraint(equalTo: searchIcon.trailingAnchor, constant: 6),
            searchLabel.centerYAnchor.constraint(equalTo: searchField.centerYAnchor),
            grid.topAnchor.constraint(equalTo: searchField.bottomAnchor, constant: 2),
            grid.bottomAnchor.constraint(equalTo: strip.topAnchor, constant: -2),
            strip.leadingAnchor.constraint(equalTo: leadingAnchor, constant: 3),
            strip.trailingAnchor.constraint(equalTo: trailingAnchor, constant: -3),
            strip.bottomAnchor.constraint(equalTo: bottomAnchor, constant: -3),
            strip.heightAnchor.constraint(equalToConstant: Self.stripHeight),
        ])
    }

    required init?(coder: NSCoder) { fatalError("init(coder:) is not used") }

    private static let symbolConfig = UIImage.SymbolConfiguration(pointSize: 17)

    /// Fills the grid: the recents (if any), then each group of `catalog`.
    func show(catalog: EmojiCatalog, history: EmojiHistory, lettersTitle: String) {
        self.catalog = catalog
        self.history = history
        makeRenderer()
        let recents = history.recents.map { EmojiItem(emoji: $0, tones: catalog.tones(of: $0)) }
        sections = (recents.isEmpty ? [] : [Section(name: "Recently used", symbol: Self.recentsSymbol, items: recents)])
            + catalog.categories.map { Section(name: $0.name, symbol: Self.symbols[$0.name] ?? "circle", items: $0.items) }
        lettersButton.setTitle(lettersTitle, for: .normal)
        strip.arrangedSubviews.forEach { $0.removeFromSuperview() }
        categoryButtons = sections.enumerated().map { index, section in
            let button = UIButton(type: .custom)
            button.setImage(UIImage(systemName: section.symbol, withConfiguration: Self.symbolConfig), for: .normal)
            button.accessibilityLabel = section.name
            button.layer.cornerRadius = 6
            button.addAction(UIAction { [weak self] _ in self?.jump(to: index) }, for: .touchUpInside)
            return button
        }
        strip.addArrangedSubview(lettersButton)
        categoryButtons.forEach(strip.addArrangedSubview)
        strip.addArrangedSubview(backspaceButton)
        for button in [lettersButton, backspaceButton] {
            button.widthAnchor.constraint(equalToConstant: 50).isActive = true
        }
        for button in categoryButtons.dropFirst() {
            button.widthAnchor.constraint(equalTo: categoryButtons[0].widthAnchor).isActive = true
        }
        grid.reloadData()
        grid.setContentOffset(.zero, animated: false)
        select(0)
        // The first screen's cells ask for their pictures now, not at the next layout.
        superview?.layoutIfNeeded()
        noteFirstScreen()
    }

    /// Once, when the grid first has cells on screen: the emoji to wait for before logging
    /// that the first screen is drawn.
    private func noteFirstScreen() {
        guard firstScreen == nil, !sections.isEmpty else { return }
        let visible = onScreen()
        guard !visible.isEmpty else { return }
        firstScreen = Set(visible).filter { renderer?.kept($0) == nil }
    }

    private func makeRenderer() {
        guard renderer == nil else { return }
        let scale = window?.windowScene?.screen.scale ?? traitCollection.displayScale
        let renderer = EmojiRenderer(pixelSide: Int((Self.emojiSize * max(scale, 1)).rounded(.up)))
        renderer.onLog = { [log] in log.info($0) }
        renderer.onScreen = { [weak self] in self?.onScreen() ?? [] }
        if !renderer.usesPrivateFont {
            log.warning("Emoji font file unreadable: drawing with the system font, whose cache grows")
        }
        self.renderer = renderer
    }

    /// The emoji in the cells on screen, top to bottom.
    private func onScreen() -> [String] {
        grid.indexPathsForVisibleItems.sorted().map { shown(sections[$0.section].items[$0.item]) }
    }

    /// One more of the first screen drawn: the log says when the last is.
    private func drewFirstScreen(_ emoji: String) {
        guard firstScreen?.remove(emoji) != nil, firstScreen?.isEmpty == true else { return }
        log.info("Emoji panel first screen drawn \(Diagnostics.milliseconds(.now - madeAt)) after the panel was made; "
                 + "\(renderer?.drawn ?? 0) emoji drawn, \(renderer?.fontsMade ?? 0) fonts")
    }

    func apply(_ theme: KeyboardTheme) {
        self.theme = theme
        backgroundColor = theme.background
        searchField.backgroundColor = theme.key
        searchIcon.tintColor = theme.hintText
        searchLabel.textColor = theme.hintText
        lettersButton.backgroundColor = theme.specialKey
        lettersButton.setTitleColor(theme.keyText, for: .normal)
        lettersButton.setTitleColor(theme.hintText, for: .highlighted)
        backspaceButton.backgroundColor = theme.specialKey
        backspaceButton.tintColor = theme.keyText
        select(selectedSection)
        grid.reloadData()
    }

    override func layoutSubviews() {
        super.layoutSubviews()
        let width = floor(bounds.width / Self.columns)
        if layout.itemSize.width != width {
            layout.itemSize = CGSize(width: width, height: Self.rowHeight)
            layout.headerReferenceSize = CGSize(width: bounds.width, height: Self.headerHeight)
            // The leftover width goes either side of the rows.
            let inset = (bounds.width - width * Self.columns) / 2
            layout.sectionInset = UIEdgeInsets(top: 0, left: inset, bottom: 4, right: inset)
            layout.invalidateLayout()
        }
        if firstScreen == nil, !sections.isEmpty {
            grid.layoutIfNeeded()
            noteFirstScreen()
        }
    }

    override func willMove(toWindow newWindow: UIWindow?) {
        super.willMove(toWindow: newWindow)
        guard newWindow == nil else { return }
        stopBackspace()
        renderer?.close()
        renderer = nil
    }

    override func didMoveToWindow() {
        super.didMoveToWindow()
        guard window != nil, renderer == nil else { return }
        makeRenderer()
        // Back in a window after leaving it: the pictures went with the old renderer.
        if !sections.isEmpty { grid.reloadData() }
    }

    // MARK: Grid

    func numberOfSections(in collectionView: UICollectionView) -> Int { sections.count }

    func collectionView(_ collectionView: UICollectionView, numberOfItemsInSection section: Int) -> Int {
        sections[section].items.count
    }

    func collectionView(_ collectionView: UICollectionView, cellForItemAt indexPath: IndexPath) -> UICollectionViewCell {
        let cell = collectionView.dequeueReusableCell(withReuseIdentifier: Self.cellID, for: indexPath)
        if let cell = cell as? EmojiCell {
            let emoji = shown(sections[indexPath.section].items[indexPath.item])
            cell.show(emoji, image: picture(for: emoji, in: cell), theme: theme)
        }
        cellsShown += 1
        if cellsShown % Self.logEvery == 0 { log.info("Emoji panel scrolled; \(statistics)") }
        return cell
    }

    private func picture(for emoji: String, in cell: EmojiCell) -> CGImage? {
        renderer?.image(for: emoji) { [weak cell, weak self] image in
            cell?.drawn(image, for: emoji)
            self?.drewFirstScreen(emoji)
        }
    }

    /// A cell made ahead (or shown again) whose picture wasn't drawn asks again: the renderer
    /// drops what has been off screen a while.
    func collectionView(_ collectionView: UICollectionView, willDisplay cell: UICollectionViewCell, forItemAt indexPath: IndexPath) {
        guard let cell = cell as? EmojiCell, !cell.hasPicture else { return }
        let emoji = shown(sections[indexPath.section].items[indexPath.item])
        if let image = picture(for: emoji, in: cell) { cell.drawn(image, for: emoji) }
    }

    func collectionView(_ collectionView: UICollectionView, prefetchItemsAt indexPaths: [IndexPath]) {
        renderer?.prefetch(indexPaths.map { shown(sections[$0.section].items[$0.item]) })
    }

    func collectionView(_ collectionView: UICollectionView, cancelPrefetchingForItemsAt indexPaths: [IndexPath]) {
        renderer?.cancelPrefetch(indexPaths.map { shown(sections[$0.section].items[$0.item]) })
    }

    /// "600 cells shown, 540 emoji drawn, 12 fonts (1 alive); footprint 25.1 MB"
    var statistics: String {
        "\(cellsShown) cells shown, \(renderer?.drawn ?? 0) emoji drawn, \(renderer?.fontsMade ?? 0) fonts "
            + "(\(EmojiRenderer.fontCopiesAlive) alive); footprint \(Diagnostics.footprint())"
    }

    /// On a memory warning: the pictures kept off screen and the font's cache.
    func releaseMemory() {
        renderer?.empty()
    }

    func collectionView(
        _ collectionView: UICollectionView, viewForSupplementaryElementOfKind kind: String, at indexPath: IndexPath
    ) -> UICollectionReusableView {
        let header = collectionView.dequeueReusableSupplementaryView(ofKind: kind, withReuseIdentifier: Self.headerID, for: indexPath)
        (header as? EmojiHeader)?.show(sections[indexPath.section].name, theme: theme)
        return header
    }

    func collectionView(_ collectionView: UICollectionView, didSelectItemAt indexPath: IndexPath) {
        collectionView.deselectItem(at: indexPath, animated: false)
        onEmoji?(shown(sections[indexPath.section].items[indexPath.item]), nil)
    }

    /// What a cell shows and a tap inserts: a group's emoji in the user's tone for it; a
    /// recent one as it was inserted.
    private func shown(_ item: EmojiItem) -> String {
        catalog?.base(of: item.emoji) == item.emoji ? history.toned(item.emoji) : item.emoji
    }

    /// The group at the top of the grid lights its symbol.
    func scrollViewDidScroll(_ scrollView: UIScrollView) {
        let top = grid.contentOffset.y + Self.headerHeight + 1
        let section = sections.indices.last { index in
            guard let header = layout.layoutAttributesForSupplementaryView(
                ofKind: UICollectionView.elementKindSectionHeader, at: IndexPath(item: 0, section: index)) else { return false }
            return header.frame.minY <= top
        } ?? 0
        if section != selectedSection { select(section) }
    }

    private func jump(to section: Int) {
        guard sections.indices.contains(section), let header = layout.layoutAttributesForSupplementaryView(
            ofKind: UICollectionView.elementKindSectionHeader, at: IndexPath(item: 0, section: section)) else { return }
        let maxY = max(0, grid.contentSize.height - grid.bounds.height)
        grid.setContentOffset(CGPoint(x: 0, y: min(header.frame.minY, maxY)), animated: false)
        select(section)
    }

    private func select(_ section: Int) {
        selectedSection = section
        for (index, button) in categoryButtons.enumerated() {
            button.tintColor = index == section ? theme?.accent : theme?.hintText
            button.accessibilityTraits = index == section ? [.button, .selected] : .button
        }
    }

    // MARK: Skin tones

    @objc private func held(_ hold: UILongPressGestureRecognizer) {
        let point = hold.location(in: self)
        switch hold.state {
        case .began:
            let inGrid = hold.location(in: grid)
            guard let path = grid.indexPathForItem(at: inGrid), let theme, let catalog,
                  let cell = grid.cellForItem(at: path) else { return }
            let item = sections[path.section].items[path.item]
            guard let base = catalog.base(of: item.emoji), !catalog.tones(of: base).isEmpty else { return }
            popupOptions = [base] + catalog.tones(of: base)
            popupIndex = popupOptions.firstIndex(of: shown(item)) ?? 0
            let rect = cell.convert(cell.bounds, to: self)
            popupCellWidth = rect.width
            _ = popup.show(popupOptions, anchor: popupIndex, above: rect, within: bounds, theme: theme)
            // Below the emoji when there is no room above it (the top row).
            if popup.frame.minY < -Self.rowHeight / 2 { popup.frame.origin.y = rect.maxY + 4 }
        case .changed:
            guard !popupOptions.isEmpty, popupCellWidth > 0 else { return }
            let index = Int(((point.x - popup.frame.minX) / popupCellWidth).rounded(.down))
            popupIndex = min(max(index, 0), popupOptions.count - 1)
            popup.highlight(popupIndex)
        case .ended:
            guard popupOptions.indices.contains(popupIndex) else { return }
            let chosen = popupOptions[popupIndex], base = popupOptions[0]
            history.chooseTone(chosen, for: base)
            closePopup()
            grid.reconfigureItems(at: grid.indexPathsForVisibleItems)
            onEmoji?(chosen, base)
        default:
            closePopup()
        }
    }

    private func closePopup() {
        popup.hide()
        popupOptions = []
    }

    // MARK: Backspace

    /// One delete at once, then after a pause one every 0.1 s while held.
    private func startBackspace() {
        stopBackspace()
        onBackspace?()
        backspaceRepeat = Timer.scheduledTimer(withTimeInterval: 0.45, repeats: false) { [weak self] _ in
            MainActor.assumeIsolated {
                self?.backspaceRepeat = Timer.scheduledTimer(withTimeInterval: 0.1, repeats: true) { [weak self] _ in
                    MainActor.assumeIsolated { self?.onBackspace?() }
                }
            }
        }
    }

    private func stopBackspace() {
        backspaceRepeat?.invalidate()
        backspaceRepeat = nil
    }
}

/// One emoji in the grid (or the search's row), lit while pressed: its picture, once drawn.
final class EmojiCell: UICollectionViewCell {
    private let picture = EmojiPicture()
    private var emoji = ""
    private var theme: KeyboardTheme?

    override init(frame: CGRect) {
        super.init(frame: frame)
        contentView.addSubview(picture)
        contentView.layer.cornerRadius = 6
        isAccessibilityElement = true
    }

    required init?(coder: NSCoder) { fatalError("init(coder:) is not used") }

    override func layoutSubviews() {
        super.layoutSubviews()
        let side = EmojiPanelView.emojiSize
        // On whole points, so the picture's pixels land on the screen's.
        picture.frame = CGRect(x: ((bounds.width - side) / 2).rounded(), y: ((bounds.height - side) / 2).rounded(),
                               width: side, height: side)
    }

    func show(_ emoji: String, image: CGImage?, theme: KeyboardTheme?) {
        self.emoji = emoji
        picture.image = image
        picture.placeholder = theme?.hintText.withAlphaComponent(0.18)
        accessibilityLabel = emoji
        self.theme = theme
        isHighlighted = false
    }

    var hasPicture: Bool { picture.image != nil }

    /// A picture that was still being drawn when the cell was filled.
    func drawn(_ image: CGImage, for emoji: String) {
        if emoji == self.emoji { picture.image = image }
    }

    override var isHighlighted: Bool {
        didSet { contentView.backgroundColor = isHighlighted ? theme?.specialKey : .clear }
    }
}

/// Draws its picture into its own layer, which the cell keeps and reuses: one backing store
/// per cell, whatever the pictures kept. A faint dot until there is a picture.
final class EmojiPicture: UIView {
    var image: CGImage? {
        didSet { if image !== oldValue { setNeedsDisplay() } }
    }
    var placeholder: UIColor? {
        didSet { if image == nil && placeholder != oldValue { setNeedsDisplay() } }
    }

    override init(frame: CGRect) {
        super.init(frame: frame)
        isOpaque = false
        backgroundColor = .clear
        isUserInteractionEnabled = false
    }

    required init?(coder: NSCoder) { fatalError("init(coder:) is not used") }

    override func draw(_ rect: CGRect) {
        guard let context = UIGraphicsGetCurrentContext() else { return }
        guard let image else {
            placeholder?.setFill()
            let side = bounds.width / 4
            context.fillEllipse(in: CGRect(x: bounds.midX - side / 2, y: bounds.midY - side / 2, width: side, height: side))
            return
        }
        // Core Graphics draws upside down in UIKit's coordinates.
        context.translateBy(x: 0, y: bounds.height)
        context.scaleBy(x: 1, y: -1)
        context.draw(image, in: bounds)
    }
}

/// A group's name above its emoji.
private final class EmojiHeader: UICollectionReusableView {
    private let label = UILabel()

    override init(frame: CGRect) {
        super.init(frame: frame)
        label.font = .systemFont(ofSize: 12, weight: .semibold)
        label.frame = bounds.insetBy(dx: 10, dy: 0)
        label.autoresizingMask = [.flexibleWidth, .flexibleHeight]
        addSubview(label)
    }

    required init?(coder: NSCoder) { fatalError("init(coder:) is not used") }

    func show(_ name: String, theme: KeyboardTheme?) {
        label.text = name.uppercased()
        label.textColor = theme?.hintText
    }
}
