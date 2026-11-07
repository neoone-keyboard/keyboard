// Renders the NEO1 app icon: N, E, O and 1 key caps, two by two, on the keyboard's grey, with
// the 1 lit in the indigo accent. Run it through tools/icon/render.sh, which downloads the font.
//
//   swift tools/icon/render.swift <SpaceGrotesk.ttf> <variant> <size> <out.png>
//
// variant: light (the icon), dark (iOS 18 dark appearance), tinted (iOS 18 tinted appearance).
// The PNG is opaque with square corners: iOS masks the corners itself.
//
// The letters are Space Grotesk Bold (SIL Open Font License 1.1). The font is used only to
// draw these pictures; it isn't in the repo or the app, and the OFL puts no condition on
// artwork made with it.
//
// Geometry is from the design's 240 pt icon (design-ref Logo.dc.html): 26 padding, 14 gap,
// 20 corner radius, a 5 pt bottom edge under each cap, 54 pt letters.

import AppKit
import CoreText
import Foundation

struct Palette {
    let background: CGColor
    let cap: CGColor
    let capEdge: CGColor
    let letter: CGColor
    let lit: CGColor
    let litEdge: CGColor
    let litLetter: CGColor
}

func rgb(_ hex: UInt32) -> CGColor {
    CGColor(srgbRed: CGFloat((hex >> 16) & 0xFF) / 255, green: CGFloat((hex >> 8) & 0xFF) / 255,
            blue: CGFloat(hex & 0xFF) / 255, alpha: 1)
}

// The accent (KeyboardTheme's indigo, #5459CC) and its edge at 68 % (#393D8B), as in the design.
let palettes: [String: Palette] = [
    "light": Palette(background: rgb(0xD1D4DB), cap: rgb(0xFFFFFF), capEdge: rgb(0xA9AEB8), letter: rgb(0x16181D),
                     lit: rgb(0x5459CC), litEdge: rgb(0x393D8B), litLetter: rgb(0xFFFFFF)),
    // The keyboard's dark look (design-ref Logo.dc.html, "on dark").
    "dark": Palette(background: rgb(0x141414), cap: rgb(0x424242), capEdge: rgb(0x262626), letter: rgb(0xF5F5F5),
                    lit: rgb(0x5459CC), litEdge: rgb(0x393D8B), litLetter: rgb(0xFFFFFF)),
    // Greys only: iOS tints by brightness, so the 1 stays the brightest cap.
    "tinted": Palette(background: rgb(0x000000), cap: rgb(0x3C3C3C), capEdge: rgb(0x1E1E1E), letter: rgb(0xD8D8D8),
                      lit: rgb(0xF2F2F2), litEdge: rgb(0x9A9A9A), litLetter: rgb(0x111111)),
]

let arguments = CommandLine.arguments
guard arguments.count == 5, let palette = palettes[arguments[2]], let size = Int(arguments[3]), size > 0 else {
    FileHandle.standardError.write("usage: render.swift <font.ttf> <light|dark|tinted> <size> <out.png>\n".data(using: .utf8)!)
    exit(2)
}
let fontURL = URL(fileURLWithPath: arguments[1])
let outURL = URL(fileURLWithPath: arguments[4])

guard let descriptors = CTFontManagerCreateFontDescriptorsFromURL(fontURL as CFURL) as? [CTFontDescriptor],
      let base = descriptors.first else {
    FileHandle.standardError.write("cannot read \(fontURL.path)\n".data(using: .utf8)!)
    exit(1)
}
let scale = CGFloat(size) / 240
// Bold: the variable font's weight axis at 700.
let wghtTag = 0x7767_6874 // 'wght'
let bold = CTFontDescriptorCreateCopyWithVariation(base, wghtTag as CFNumber, 700)
let font = CTFontCreateWithFontDescriptor(bold, 54 * scale, nil)

let space = CGColorSpace(name: CGColorSpace.sRGB)!
guard let context = CGContext(data: nil, width: size, height: size, bitsPerComponent: 8, bytesPerRow: 0, space: space,
                              bitmapInfo: CGImageAlphaInfo.noneSkipLast.rawValue) else { exit(1) }
// Top-left origin, as in the design.
context.translateBy(x: 0, y: CGFloat(size))
context.scaleBy(x: 1, y: -1)

context.setFillColor(palette.background)
context.fill(CGRect(x: 0, y: 0, width: size, height: size))

let padding = 26 * scale
let gap = 14 * scale
let cap = (CGFloat(size) - 2 * padding - gap) / 2
let radius = 20 * scale
let edge = 5 * scale

func drawCap(_ letter: String, column: Int, row: Int, lit: Bool) {
    let rect = CGRect(x: padding + CGFloat(column) * (cap + gap), y: padding + CGFloat(row) * (cap + gap),
                      width: cap, height: cap)
    // The bottom edge: the same cap, lower, in the darker shade.
    context.setFillColor(lit ? palette.litEdge : palette.capEdge)
    context.addPath(CGPath(roundedRect: rect.offsetBy(dx: 0, dy: edge), cornerWidth: radius, cornerHeight: radius, transform: nil))
    context.fillPath()
    context.setFillColor(lit ? palette.lit : palette.cap)
    context.addPath(CGPath(roundedRect: rect, cornerWidth: radius, cornerHeight: radius, transform: nil))
    context.fillPath()

    // The letter, centred on its ink: capitals and figures sit on the baseline with no
    // descender, so the glyph bounds centre them optically.
    let attributed = NSAttributedString(string: letter, attributes: [
        NSAttributedString.Key(kCTFontAttributeName as String): font,
        NSAttributedString.Key(kCTForegroundColorAttributeName as String): lit ? palette.litLetter : palette.letter,
    ])
    let line = CTLineCreateWithAttributedString(attributed)
    context.textPosition = .zero
    let ink = CTLineGetImageBounds(line, context)
    context.saveGState()
    // CoreText draws with y up: flip locally around the letter.
    context.translateBy(x: rect.midX, y: rect.midY)
    context.scaleBy(x: 1, y: -1)
    context.textPosition = CGPoint(x: -ink.midX, y: -ink.midY)
    CTLineDraw(line, context)
    context.restoreGState()
}

drawCap("N", column: 0, row: 0, lit: false)
drawCap("E", column: 1, row: 0, lit: false)
drawCap("O", column: 0, row: 1, lit: false)
drawCap("1", column: 1, row: 1, lit: true)

guard let image = context.makeImage() else { exit(1) }
let bitmap = NSBitmapImageRep(cgImage: image)
guard let png = bitmap.representation(using: .png, properties: [:]) else { exit(1) }
try png.write(to: outURL)
