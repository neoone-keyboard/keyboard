import Foundation

/// Word → emoji for the bar, from the bundled emoji-words.txt (data/pipeline/emoji_data.py):
/// comment lines, then "word<TAB>emoji emoji…" lines sorted by UTF-8 bytes, best emoji first.
/// Looked up by binary search on the file's bytes, so nothing is parsed or held but the
/// file itself (memory-mapped by the caller); a lookup reads a dozen lines.
public struct EmojiWords: Sendable {
    private let data: Data
    /// Where the first word line starts, after the comments.
    private let bodyStart: Int

    public init(data: Data) {
        self.data = data
        var start = 0
        data.withUnsafeBytes { (bytes: UnsafeRawBufferPointer) in
            while start < bytes.count, bytes[start] == UInt8(ascii: "#") {
                while start < bytes.count, bytes[start] != UInt8(ascii: "\n") { start += 1 }
                start += 1
            }
        }
        bodyStart = min(start, data.count)
    }

    /// The emoji for `word` (any case; ’ as '), best first; none for a word not in the list.
    public func emoji(for word: String) -> [String] {
        let key = Array(word.lowercased().replacingOccurrences(of: "\u{2019}", with: "'").utf8)
        guard !key.isEmpty else { return [] }
        return data.withUnsafeBytes { (bytes: UnsafeRawBufferPointer) -> [String] in
            let newline = UInt8(ascii: "\n"), tab = UInt8(ascii: "\t")
            // `low` and `high` are always at line starts.
            var low = bodyStart, high = bytes.count
            while low < high {
                let mid = (low + high) / 2
                var start = mid
                while start > low, bytes[start - 1] != newline { start -= 1 }
                var end = start
                while end < high, bytes[end] != newline { end += 1 }
                var split = start
                while split < end, bytes[split] != tab { split += 1 }
                let order = compare(key, bytes[start..<split])
                if order < 0 {
                    high = start
                } else if order > 0 {
                    low = end + 1
                } else {
                    guard split < end else { return [] }
                    return String(decoding: bytes[(split + 1)..<end], as: UTF8.self)
                        .split(separator: " ").map(String.init)
                }
            }
            return []
        }
    }

    /// Byte order of `key` against `word`: negative, zero or positive.
    private func compare(_ key: [UInt8], _ word: Slice<UnsafeRawBufferPointer>) -> Int {
        for (a, b) in zip(key, word) where a != b {
            return a < b ? -1 : 1
        }
        return key.count - word.count
    }
}

/// The emoji in the bar's right slot: one for the word being typed, or else for the word a
/// space has just finished, as SwiftKey offers 🍕 for "pizza".
public enum EmojiSuggestion {
    /// The word the bar's emoji is for: `currentWord` while typing it; after one space, the
    /// word before it; otherwise none.
    public static func word(contextBefore: String, currentWord: String) -> String? {
        if !currentWord.isEmpty { return currentWord }
        guard contextBefore.last == " " else { return nil }
        let before = contextBefore.dropLast()
        let word = String(before.reversed().prefix { $0.isLetter || $0.isNumber || $0 == "'" || $0 == "\u{2019}" }.reversed())
        return word.contains(where: \.isLetter) ? word : nil
    }

    /// Of `candidates` (best first), the one the user picked most recently, else the best,
    /// in the user's skin tone for it.
    public static func pick(_ candidates: [String], history: EmojiHistory) -> String? {
        let keys = candidates.map(EmojiSupport.toneless)
        let recent = history.recents.lazy.compactMap { keys.firstIndex(of: EmojiSupport.toneless($0)) }.first
        guard let index = recent ?? candidates.indices.first else { return nil }
        return history.toned(candidates[index])
    }
}
