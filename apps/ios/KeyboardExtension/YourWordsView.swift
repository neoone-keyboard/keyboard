import UIKit
import KeyboardCore

/// "Your words", opened from the settings panel: the learned words, each with a button that
/// forgets it, and the words removed from the bar, each with one that restores it. It takes
/// the top bar's place above the keys, which stay showing: a keyboard can't type into its own
/// text field, so while this is open the controller gives the keys to `handle` and they type
/// the search. The lists are held only while it is open (`release`).
final class YourWordsView: UIView, UITableViewDataSource, UITableViewDelegate {
    var onBack: (@MainActor () -> Void)?
    var onForget: (@MainActor (String) -> Void)?
    var onRestore: (@MainActor (String) -> Void)?

    /// Nil until the engine's lists arrive, and again once released.
    private var words: YourWords?
    private var theme: KeyboardTheme?

    private let backButton = UIButton(type: .system)
    private let searchBox = UIView()
    private let searchIcon = UIImageView(image: UIImage(systemName: "magnifyingglass"))
    private let searchLabel = UILabel()
    private let clearButton = UIButton(type: .system)
    private let table = UITableView(frame: .zero, style: .plain)

    private enum Section: Int, CaseIterable {
        case learned, removed
    }

    override init(frame: CGRect) {
        super.init(frame: frame)
        layer.cornerRadius = 8

        backButton.setTitle("Settings", for: .normal)
        backButton.setImage(UIImage(systemName: "chevron.left"), for: .normal)
        backButton.titleLabel?.font = .systemFont(ofSize: 17, weight: .semibold)
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
        header.spacing = 10
        header.alignment = .center
        header.translatesAutoresizingMaskIntoConstraints = false

        table.dataSource = self
        table.delegate = self
        table.register(WordCell.self, forCellReuseIdentifier: WordCell.reuseID)
        table.register(SectionHeader.self, forHeaderFooterViewReuseIdentifier: SectionHeader.reuseID)
        table.rowHeight = 38
        table.sectionHeaderHeight = 24
        table.sectionHeaderTopPadding = 0
        table.estimatedRowHeight = 0
        table.estimatedSectionHeaderHeight = 0
        table.backgroundColor = .clear
        table.separatorInset = UIEdgeInsets(top: 0, left: 12, bottom: 0, right: 12)
        // iOS 26 blurs scroll content near each edge, which smears the rows in a short list.
        for edge in [table.topEdgeEffect, table.bottomEdgeEffect, table.leftEdgeEffect, table.rightEdgeEffect] {
            edge.isHidden = true
        }
        table.translatesAutoresizingMaskIntoConstraints = false

        addSubview(header)
        addSubview(table)
        NSLayoutConstraint.activate([
            header.leadingAnchor.constraint(equalTo: leadingAnchor, constant: 8),
            header.trailingAnchor.constraint(equalTo: trailingAnchor, constant: -8),
            header.topAnchor.constraint(equalTo: topAnchor, constant: 4),
            header.heightAnchor.constraint(equalToConstant: 36),
            searchBox.heightAnchor.constraint(equalToConstant: 34),
            searchRow.leadingAnchor.constraint(equalTo: searchBox.leadingAnchor, constant: 8),
            searchRow.trailingAnchor.constraint(equalTo: searchBox.trailingAnchor, constant: -6),
            searchRow.topAnchor.constraint(equalTo: searchBox.topAnchor),
            searchRow.bottomAnchor.constraint(equalTo: searchBox.bottomAnchor),
            table.leadingAnchor.constraint(equalTo: leadingAnchor),
            table.trailingAnchor.constraint(equalTo: trailingAnchor),
            table.topAnchor.constraint(equalTo: header.bottomAnchor, constant: 4),
            table.bottomAnchor.constraint(equalTo: bottomAnchor),
        ])
        refreshSearch()
    }

    required init?(coder: NSCoder) { fatalError("init(coder:) is not used") }

    /// Shows the engine's lists (nil while they load), with an empty search, from the top.
    func show(_ words: YourWords?) {
        self.words = words
        table.reloadData()
        table.setContentOffset(.zero, animated: false)
        refreshSearch()
    }

    /// Drops the lists and the cells drawn for them; the screen is closed.
    func release() {
        words = nil
        table.reloadData()
    }

    /// A key tapped while the screen is open.
    func handle(_ action: KeyAction) -> YourWords.KeyResult {
        guard var words else {
            // Still loading: only the keys that change the keys do anything.
            var loading = YourWords(learned: [], removed: [])
            return loading.handle(action) == .passThrough ? .passThrough : .ignored
        }
        let result = words.handle(action)
        update(words)
        return result
    }

    /// A long-press alternate, typed into the search.
    func type(_ text: String) {
        guard var words else { return }
        words.type(text)
        update(words)
    }

    func clearQuery() {
        guard var words else { return }
        words.clearQuery()
        update(words)
    }

    func apply(_ theme: KeyboardTheme) {
        self.theme = theme
        overrideUserInterfaceStyle = theme.isDark ? .dark : .light
        backgroundColor = theme.background
        backButton.tintColor = theme.keyText
        searchBox.backgroundColor = theme.key
        searchIcon.tintColor = theme.hintText
        clearButton.tintColor = theme.hintText
        table.separatorColor = theme.specialKey
        refreshSearch()
        table.reloadData()
    }

    private func update(_ changed: YourWords) {
        let queryChanged = changed.query != words?.query
        words = changed
        refreshSearch()
        guard queryChanged else { return }
        table.reloadData()
        table.setContentOffset(.zero, animated: false)
    }

    private func refreshSearch() {
        let query = words?.query ?? ""
        if query.isEmpty {
            searchLabel.text = "Type to search"
            searchLabel.textColor = theme?.hintText ?? .secondaryLabel
        } else {
            searchLabel.text = query
            searchLabel.textColor = theme?.keyText ?? .label
        }
        searchLabel.accessibilityLabel = query.isEmpty ? "Search your words" : "Search: \(query)"
        clearButton.isHidden = query.isEmpty
    }

    /// The words a section lists; empty while loading.
    private func list(_ section: Section) -> [String] {
        switch section {
        case .learned: words?.learned ?? []
        case .removed: words?.removed ?? []
        }
    }

    /// What an empty section says in its one row; nil when it lists words.
    private func emptyMessage(_ section: Section) -> String? {
        guard let words else { return section == .learned ? "Loading…" : nil }
        switch section {
        case .learned: return words.emptyLearnedMessage
        case .removed: return words.emptyRemovedMessage
        }
    }

    private func forget(_ word: String) {
        guard var words else { return }
        words.forget(word)
        update(words)
        table.reloadSections(IndexSet(integer: Section.learned.rawValue), with: .none)
        onForget?(word)
    }

    private func restore(_ word: String) {
        guard var words else { return }
        words.restore(word)
        update(words)
        table.reloadSections(IndexSet(integer: Section.removed.rawValue), with: .none)
        onRestore?(word)
    }

    // MARK: UITableViewDataSource, UITableViewDelegate

    func numberOfSections(in tableView: UITableView) -> Int {
        Section.allCases.count
    }

    func tableView(_ tableView: UITableView, numberOfRowsInSection index: Int) -> Int {
        guard let section = Section(rawValue: index) else { return 0 }
        let count = list(section).count
        return count > 0 ? count : (emptyMessage(section) == nil ? 0 : 1)
    }

    func tableView(_ tableView: UITableView, cellForRowAt indexPath: IndexPath) -> UITableViewCell {
        let cell = tableView.dequeueReusableCell(withIdentifier: WordCell.reuseID, for: indexPath)
        guard let cell = cell as? WordCell, let section = Section(rawValue: indexPath.section),
              let theme else { return cell }
        let words = list(section)
        guard words.indices.contains(indexPath.row) else {
            cell.showMessage(emptyMessage(section) ?? "", theme: theme)
            return cell
        }
        let word = words[indexPath.row]
        switch section {
        case .learned:
            cell.show(word, action: .forget, theme: theme) { [weak self] in self?.forget(word) }
        case .removed:
            cell.show(word, action: .restore, theme: theme) { [weak self] in self?.restore(word) }
        }
        return cell
    }

    func tableView(_ tableView: UITableView, viewForHeaderInSection index: Int) -> UIView? {
        let header = tableView.dequeueReusableHeaderFooterView(withIdentifier: SectionHeader.reuseID)
        guard let header = header as? SectionHeader, let section = Section(rawValue: index),
              let theme else { return header }
        let title = section == .learned ? "Learned" : "Removed from suggestions"
        let count = list(section).count
        header.show(count > 0 ? "\(title) · \(count)" : title, theme: theme)
        return header
    }

    func tableView(_ tableView: UITableView, shouldHighlightRowAt indexPath: IndexPath) -> Bool {
        false
    }
}

/// One word and its button (forget, or restore), or a section's empty-state message.
private final class WordCell: UITableViewCell {
    static let reuseID = "word"

    enum Action {
        case forget, restore
    }

    private let wordLabel = UILabel()
    private let actionButton = UIButton(type: .system)
    private var onAction: (@MainActor () -> Void)?

    override init(style: UITableViewCell.CellStyle, reuseIdentifier: String?) {
        super.init(style: style, reuseIdentifier: reuseIdentifier)
        backgroundColor = .clear
        selectionStyle = .none
        wordLabel.font = .systemFont(ofSize: 17)
        wordLabel.lineBreakMode = .byTruncatingMiddle
        actionButton.setContentHuggingPriority(.required, for: .horizontal)
        actionButton.addAction(UIAction { [weak self] _ in self?.onAction?() }, for: .touchUpInside)
        let row = UIStackView(arrangedSubviews: [wordLabel, actionButton])
        row.spacing = 8
        row.alignment = .center
        row.translatesAutoresizingMaskIntoConstraints = false
        contentView.addSubview(row)
        NSLayoutConstraint.activate([
            row.leadingAnchor.constraint(equalTo: contentView.leadingAnchor, constant: 12),
            row.trailingAnchor.constraint(equalTo: contentView.trailingAnchor, constant: -8),
            row.topAnchor.constraint(equalTo: contentView.topAnchor),
            row.bottomAnchor.constraint(equalTo: contentView.bottomAnchor),
            // A comfortable target at the row's full height.
            actionButton.widthAnchor.constraint(equalToConstant: 44),
            actionButton.heightAnchor.constraint(equalTo: row.heightAnchor),
        ])
    }

    required init?(coder: NSCoder) { fatalError("init(coder:) is not used") }

    override func prepareForReuse() {
        super.prepareForReuse()
        onAction = nil
    }

    func show(_ word: String, action: Action, theme: KeyboardTheme, onAction: @escaping @MainActor () -> Void) {
        self.onAction = onAction
        wordLabel.text = word
        wordLabel.textColor = theme.keyText
        actionButton.isHidden = false
        actionButton.tintColor = theme.keyText
        switch action {
        case .forget:
            actionButton.setImage(UIImage(systemName: "trash"), for: .normal)
            actionButton.accessibilityLabel = "Forget \(word)"
        case .restore:
            actionButton.setImage(UIImage(systemName: "arrow.uturn.backward.circle"), for: .normal)
            actionButton.accessibilityLabel = "Restore \(word)"
        }
    }

    func showMessage(_ message: String, theme: KeyboardTheme) {
        onAction = nil
        wordLabel.text = message
        wordLabel.textColor = theme.hintText
        actionButton.isHidden = true
    }
}

private final class SectionHeader: UITableViewHeaderFooterView {
    static let reuseID = "section"

    private let titleLabel = UILabel()

    override init(reuseIdentifier: String?) {
        super.init(reuseIdentifier: reuseIdentifier)
        titleLabel.font = .systemFont(ofSize: 13, weight: .semibold)
        titleLabel.translatesAutoresizingMaskIntoConstraints = false
        contentView.addSubview(titleLabel)
        NSLayoutConstraint.activate([
            titleLabel.leadingAnchor.constraint(equalTo: contentView.leadingAnchor, constant: 12),
            titleLabel.trailingAnchor.constraint(equalTo: contentView.trailingAnchor, constant: -12),
            titleLabel.centerYAnchor.constraint(equalTo: contentView.centerYAnchor),
        ])
    }

    required init?(coder: NSCoder) { fatalError("init(coder:) is not used") }

    func show(_ title: String, theme: KeyboardTheme) {
        titleLabel.text = title
        titleLabel.textColor = theme.hintText
        // Sticky headers sit over the rows, so they need the screen's own background.
        var background = UIBackgroundConfiguration.clear()
        background.backgroundColor = theme.background
        backgroundConfiguration = background
    }
}
