import UIKit
import KeyboardCore

/// The emoji panel's search, opened from its search field: the query, and a row of the emoji
/// it finds that scrolls sideways; tapping one types it. Like "Your words" it takes the top
/// bar's place above the keys, which type the query (`handle`). The search and its pictures
/// are held only while it is open (`release`).
final class EmojiSearchView: UIView, UICollectionViewDataSource, UICollectionViewDelegate {
    /// Back to the panel's grid.
    var onBack: (@MainActor () -> Void)?
    var onPick: (@MainActor (String) -> Void)?

    private var search: EmojiSearch?
    private var renderer: EmojiRenderer?
    private var theme: KeyboardTheme?

    private let backButton = UIButton(type: .system)
    private let searchBox = UIView()
    private let searchIcon = UIImageView(image: UIImage(systemName: "magnifyingglass"))
    private let searchLabel = UILabel()
    private let clearButton = UIButton(type: .system)
    private let layout = UICollectionViewFlowLayout()
    private lazy var row = UICollectionView(frame: .zero, collectionViewLayout: layout)
    private let emptyLabel = UILabel()

    /// How much taller than the top bar it is: the row of emoji.
    static let extraHeight: CGFloat = 52
    private static let cellID = "emoji"

    override init(frame: CGRect) {
        super.init(frame: frame)
        layer.cornerRadius = 8

        backButton.setImage(UIImage(systemName: "chevron.left"), for: .normal)
        backButton.accessibilityLabel = "Back to emoji"
        backButton.setContentHuggingPriority(.required, for: .horizontal)
        backButton.setContentCompressionResistancePriority(.required, for: .horizontal)
        backButton.addAction(UIAction { [weak self] _ in self?.onBack?() }, for: .touchUpInside)

        // Not a text field: the keys below type into it (see the type's comment).
        searchBox.layer.cornerRadius = 8
        searchIcon.contentMode = .scaleAspectFit
        searchIcon.setContentHuggingPriority(.required, for: .horizontal)
        searchLabel.font = .systemFont(ofSize: 16)
        searchLabel.lineBreakMode = .byTruncatingHead
        searchLabel.accessibilityTraits = .searchField
        clearButton.setImage(UIImage(systemName: "xmark.circle.fill"), for: .normal)
        clearButton.accessibilityLabel = "Clear search"
        clearButton.setContentHuggingPriority(.required, for: .horizontal)
        clearButton.addAction(UIAction { [weak self] _ in self?.clearQuery() }, for: .touchUpInside)
        let searchRow = UIStackView(arrangedSubviews: [searchIcon, searchLabel, clearButton])
        searchRow.spacing = 6
        searchRow.alignment = .center
        searchRow.translatesAutoresizingMaskIntoConstraints = false
        searchBox.addSubview(searchRow)

        let header = UIStackView(arrangedSubviews: [backButton, searchBox])
        header.spacing = 6
        header.alignment = .center
        header.translatesAutoresizingMaskIntoConstraints = false

        layout.scrollDirection = .horizontal
        layout.itemSize = CGSize(width: 46, height: 48)
        layout.minimumLineSpacing = 0
        layout.sectionInset = UIEdgeInsets(top: 0, left: 4, bottom: 0, right: 4)
        row.dataSource = self
        row.delegate = self
        row.backgroundColor = .clear
        row.showsHorizontalScrollIndicator = false
        row.register(EmojiCell.self, forCellWithReuseIdentifier: Self.cellID)
        // iOS 26 blurs scroll content near each edge, which smears the emoji at the ends.
        for edge in [row.topEdgeEffect, row.bottomEdgeEffect, row.leftEdgeEffect, row.rightEdgeEffect] {
            edge.isHidden = true
        }
        row.translatesAutoresizingMaskIntoConstraints = false
        emptyLabel.font = .systemFont(ofSize: 15)
        emptyLabel.textAlignment = .center
        emptyLabel.translatesAutoresizingMaskIntoConstraints = false

        addSubview(header)
        addSubview(row)
        addSubview(emptyLabel)
        NSLayoutConstraint.activate([
            header.leadingAnchor.constraint(equalTo: leadingAnchor, constant: 4),
            header.trailingAnchor.constraint(equalTo: trailingAnchor, constant: -8),
            header.topAnchor.constraint(equalTo: topAnchor, constant: 2),
            header.heightAnchor.constraint(equalToConstant: 36),
            backButton.widthAnchor.constraint(equalToConstant: 32),
            searchBox.heightAnchor.constraint(equalToConstant: 34),
            searchRow.leadingAnchor.constraint(equalTo: searchBox.leadingAnchor, constant: 8),
            searchRow.trailingAnchor.constraint(equalTo: searchBox.trailingAnchor, constant: -6),
            searchRow.topAnchor.constraint(equalTo: searchBox.topAnchor),
            searchRow.bottomAnchor.constraint(equalTo: searchBox.bottomAnchor),
            row.leadingAnchor.constraint(equalTo: leadingAnchor),
            row.trailingAnchor.constraint(equalTo: trailingAnchor),
            row.topAnchor.constraint(equalTo: header.bottomAnchor, constant: 2),
            row.heightAnchor.constraint(equalToConstant: 48),
            emptyLabel.centerXAnchor.constraint(equalTo: row.centerXAnchor),
            emptyLabel.centerYAnchor.constraint(equalTo: row.centerYAnchor),
        ])
        refresh()
    }

    required init?(coder: NSCoder) { fatalError("init(coder:) is not used") }

    /// Opens with an empty query (the recents).
    func show(_ search: EmojiSearch) {
        self.search = search
        let scale = window?.windowScene?.screen.scale ?? traitCollection.displayScale
        renderer = EmojiRenderer(pixelSide: Int((EmojiPanelView.emojiSize * max(scale, 1)).rounded(.up)), cacheBytes: 1_048_576)
        reload()
    }

    /// Drops the search and its pictures; it is closed.
    func release() {
        search = nil
        renderer?.close()
        renderer = nil
        row.reloadData()
    }

    func releaseMemory() {
        renderer?.empty()
    }

    /// A key tapped while the search is open.
    func handle(_ action: KeyAction) -> EmojiSearch.KeyResult {
        guard var search else { return .ignored }
        let result = search.handle(action)
        update(search)
        return result
    }

    /// A long-press alternate, typed into the query.
    func type(_ text: String) {
        guard var search else { return }
        search.type(text)
        update(search)
    }

    func clearQuery() {
        guard var search else { return }
        search.clearQuery()
        update(search)
    }

    func apply(_ theme: KeyboardTheme) {
        self.theme = theme
        overrideUserInterfaceStyle = theme.isDark ? .dark : .light
        backgroundColor = theme.background
        backButton.tintColor = theme.keyText
        searchBox.backgroundColor = theme.key
        searchIcon.tintColor = theme.hintText
        clearButton.tintColor = theme.hintText
        emptyLabel.textColor = theme.hintText
        refresh()
        row.reloadData()
    }

    private func update(_ changed: EmojiSearch) {
        let resultsChanged = changed.results != search?.results
        search = changed
        if resultsChanged { reload() } else { refresh() }
    }

    private func reload() {
        row.reloadData()
        row.setContentOffset(.zero, animated: false)
        refresh()
    }

    private func refresh() {
        let query = search?.query ?? ""
        if query.isEmpty {
            searchLabel.text = "Search emoji"
            searchLabel.textColor = theme?.hintText ?? .secondaryLabel
        } else {
            searchLabel.text = query
            searchLabel.textColor = theme?.keyText ?? .label
        }
        searchLabel.accessibilityLabel = query.isEmpty ? "Search emoji" : "Search emoji: \(query)"
        clearButton.isHidden = query.isEmpty
        let empty = search?.results.isEmpty ?? true
        emptyLabel.isHidden = !empty
        emptyLabel.text = query.isEmpty ? "Type to find emoji" : "No emoji found"
    }

    // MARK: Row

    func collectionView(_ collectionView: UICollectionView, numberOfItemsInSection section: Int) -> Int {
        search?.results.count ?? 0
    }

    func collectionView(_ collectionView: UICollectionView, cellForItemAt indexPath: IndexPath) -> UICollectionViewCell {
        let cell = collectionView.dequeueReusableCell(withReuseIdentifier: Self.cellID, for: indexPath)
        if let cell = cell as? EmojiCell, let emoji = search?.results[indexPath.item] {
            let image = renderer?.image(for: emoji) { [weak cell] image in cell?.drawn(image, for: emoji) }
            cell.show(emoji, image: image, theme: theme)
        }
        return cell
    }

    func collectionView(_ collectionView: UICollectionView, didSelectItemAt indexPath: IndexPath) {
        collectionView.deselectItem(at: indexPath, animated: false)
        guard let emoji = search?.results[indexPath.item] else { return }
        onPick?(emoji)
    }
}
