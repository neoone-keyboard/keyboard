import Foundation

/// Why the engine can't use the pack folder at `packDir`, or nil: what the app's installer
/// asks before it puts a pack where the keyboard looks. First `packProblem` (the manifest, the
/// script's rules, that there is a lexicon file), then the pack is opened as the keyboard
/// opens it, in an engine of its own with nothing else, so a lexicon the engine can't read
/// (not an FST, or one whose checksum fails) is found here and not by the keyboard. The files
/// are mapped, not read, and the engine is dropped at once.
///
/// The optional files (the models, the romanised lexicon, the next words) are not part of
/// the answer: the engine leaves out one it can't read and uses the rest of the pack.
public func packOpenProblem(packDir: String) throws -> String? {
    if let why = try packProblem(packDir: packDir) { return why }
    let engine = try Engine(config: EngineConfig(packDirs: [packDir]))
    guard let problem = engine.packProblems().first else { return nil }
    // "<folder>: <reason>"; the folder is the installer's working one, of no use to the user.
    let folder = URL(fileURLWithPath: packDir).lastPathComponent + ": "
    return problem.hasPrefix(folder) ? String(problem.dropFirst(folder.count)) : problem
}
