import UIKit
import KeyboardCore

/// The emoji panel, over the keys at their height: the recents and Unicode's groups in one
/// grid that scrolls down, and along the bottom ABC (back to the letters), a symbol per group
/// that jumps the grid to it, and backspace (repeats while held). Holding an emoji that takes
/// skin tones shows them in the alternates popup; sliding picks one. The grid's cells are
/// reused and show pictures from `EmojiRenderer`, drawn off the main thread, not text: text
/// would keep every emoji ever shown in the system font's cache. Made when opened and dropped
/// on close, with the catalog and the pictures.
final class EmojiPanelView: UIView, UICollectionViewDataSource, UICollectionViewDataSourcePrefetching,
    UICollectionViewDelegate {
    /// An emoji was picked: `emoji` as inserted, and its `base` when a tone was chosen for it.
    var onEmoji: (@MainActor (_ emoji: String, _ toneFor: String?) -> Void)?
    var onBackspace: (@MainActor () -> Void)?
    var onLetters: (@MainActor () -> Void)?

    private struct Section {
        let name: String
        let symbol: String
        let items: [EmojiItem]
    }

    private var sections: [Section] = []
    private var catalog: EmojiCatalog?
    private var renderer: EmojiRenderer?
    /// Cells filled since the panel opened, for the diagnostics log.
    private var cellsShown = 0
    private let log = Diagnostics(category: "emoji")
    /// For each emoji's tone; the recents shown are those of when the panel opened.
    private var history = EmojiHistory()
    private var theme: KeyboardTheme?
    private let layout = UICollectionViewFlowLayout()
    private lazy var grid = UICollectionView(frame: .zero, collectionViewLayout: layout)
    private let strip = UIStackView()
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

        for view in [grid, strip] as [UIView] {
            view.translatesAutoresizingMaskIntoConstraints = false
            addSubview(view)
        }
        addSubview(popup)
        NSLayoutConstraint.activate([
            grid.leadingAnchor.constraint(equalTo: leadingAnchor),
            grid.trailingAnchor.constraint(equalTo: trailingAnchor),
            grid.topAnchor.constraint(equalTo: topAnchor),
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
        let scale = window?.windowScene?.screen.scale ?? traitCollection.displayScale
        renderer = EmojiRenderer(pixelSide: Int((Self.emojiSize * max(scale, 1)).rounded(.up)))
        if renderer?.usesPrivateFont == false {
            log.warning("Emoji font file unreadable: drawing with the system font, whose cache grows")
        }
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
    }

    func apply(_ theme: KeyboardTheme) {
        self.theme = theme
        backgroundColor = theme.background
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
    }

    override func willMove(toWindow newWindow: UIWindow?) {
        super.willMove(toWindow: newWindow)
        if newWindow == nil { stopBackspace() }
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
            let image = renderer?.image(for: emoji) { [weak cell] image in cell?.drawn(image, for: emoji) }
            cell.show(emoji, image: image, theme: theme)
        }
        cellsShown += 1
        if cellsShown % Self.logEvery == 0 { log.info("Emoji panel scrolled; \(statistics)") }
        return cell
    }

    func collectionView(_ collectionView: UICollectionView, prefetchItemsAt indexPaths: [IndexPath]) {
        renderer?.prefetch(indexPaths.map { shown(sections[$0.section].items[$0.item]) })
    }

    /// "600 cells shown, 540 emoji drawn, 22 fonts; footprint 25.1 MB"
    var statistics: String {
        "\(cellsShown) cells shown, \(renderer?.drawn ?? 0) emoji drawn, \(renderer?.fontsMade ?? 0) fonts; footprint \(Diagnostics.footprint())"
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

/// One emoji in the grid, lit while pressed: its picture, once drawn.
private final class EmojiCell: UICollectionViewCell {
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
        accessibilityLabel = emoji
        self.theme = theme
        isHighlighted = false
    }

    /// A picture that was still being drawn when the cell was filled.
    func drawn(_ image: CGImage, for emoji: String) {
        if emoji == self.emoji { picture.image = image }
    }

    override var isHighlighted: Bool {
        didSet { contentView.backgroundColor = isHighlighted ? theme?.specialKey : .clear }
    }
}

/// Draws its picture into its own layer, which the cell keeps and reuses: one backing store
/// per cell, whatever the pictures kept.
final class EmojiPicture: UIView {
    var image: CGImage? {
        didSet { if image !== oldValue { setNeedsDisplay() } }
    }

    override init(frame: CGRect) {
        super.init(frame: frame)
        isOpaque = false
        backgroundColor = .clear
        isUserInteractionEnabled = false
    }

    required init?(coder: NSCoder) { fatalError("init(coder:) is not used") }

    override func draw(_ rect: CGRect) {
        guard let image, let context = UIGraphicsGetCurrentContext() else { return }
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
