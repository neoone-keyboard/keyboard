import Foundation
import Testing
@testable import KeyboardCore

// The App Group and the pack type come from the app's and the keyboard's Info.plist, which
// project.yml fills from apps/ios/Signing.xcconfig. With no Info.plist (these tests on the Mac),
// they are the published app's.

@Test func theAppGroupIsTheOneTheInfoPlistNames() {
    #expect(PackLocations.appGroup(infoDictionary: ["NeoAppGroup": "group.com.example.neoone"]) == "group.com.example.neoone")
}

@Test func withoutAnInfoPlistKeyTheAppGroupIsThePublishedOne() {
    #expect(PackLocations.appGroup(infoDictionary: nil) == "group.me.jishnu.keyboard")
    #expect(PackLocations.appGroup(infoDictionary: ["NeoAppGroup": ""]) == "group.me.jishnu.keyboard")
    #expect(PackLocations.appGroup == PackLocations.defaultAppGroup)
}

private func declaring(_ identifier: String, extensions: [String] = ["neopack"]) -> [String: Any] {
    ["UTExportedTypeDeclarations": [
        ["UTTypeIdentifier": "com.example.other", "UTTypeTagSpecification": ["public.filename-extension": ["other"]]],
        ["UTTypeIdentifier": identifier, "UTTypeTagSpecification": ["public.filename-extension": extensions]],
    ]]
}

@Test func thePackTypeIsTheOneTheInfoPlistDeclaresForNeopackFiles() {
    #expect(PackImport.typeIdentifier(infoDictionary: declaring("com.example.neoone.neopack")) == "com.example.neoone.neopack")
}

@Test func withoutADeclarationThePackTypeIsThePublishedOne() {
    #expect(PackImport.typeIdentifier(infoDictionary: nil) == "me.jishnu.keyboard.neopack")
    #expect(PackImport.typeIdentifier(infoDictionary: declaring("com.example.zip", extensions: ["zip"])) == "me.jishnu.keyboard.neopack")
    #expect(PackImport.typeIdentifier == PackImport.defaultTypeIdentifier)
}
