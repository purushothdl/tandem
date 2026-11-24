// Draws the app icon and the menu-bar icon. Run from the project folder:
//   swift scripts/make_icons.swift && iconutil -c icns target/AppIcon.iconset -o resources/AppIcon.icns
import AppKit

func png(_ size: Int, _ draw: (CGFloat) -> Void) -> Data {
    let rep = NSBitmapImageRep(bitmapDataPlanes: nil, pixelsWide: size, pixelsHigh: size, bitsPerSample: 8,
                               samplesPerPixel: 4, hasAlpha: true, isPlanar: false, colorSpaceName: .deviceRGB,
                               bytesPerRow: 0, bitsPerPixel: 0)!
    NSGraphicsContext.saveGraphicsState()
    NSGraphicsContext.current = NSGraphicsContext(bitmapImageRep: rep)
    draw(CGFloat(size))
    NSGraphicsContext.restoreGraphicsState()
    return rep.representation(using: .png, properties: [:])!
}

// Phosphor's "intersect-square": see resources/ICON-LICENSE.txt
let glyph = NSImage(contentsOfFile: "resources/intersect-square.svg")!

func appIcon(_ s: CGFloat) {
    // macOS icons leave a margin around the body
    let m = s * 100 / 1024
    let body = NSRect(x: m, y: m, width: s - 2 * m, height: s - 2 * m)
    let path = NSBezierPath(roundedRect: body, xRadius: s * 185 / 1024, yRadius: s * 185 / 1024)
    NSGraphicsContext.saveGraphicsState()
    let shadow = NSShadow()
    shadow.shadowColor = NSColor(white: 0, alpha: 0.3)
    shadow.shadowBlurRadius = s * 20 / 1024
    shadow.shadowOffset = NSSize(width: 0, height: -s * 10 / 1024)
    shadow.set()
    NSColor.black.setFill()
    path.fill()
    NSGraphicsContext.restoreGraphicsState()
    NSGradient(starting: NSColor(red: 0.51, green: 0.46, blue: 0.98, alpha: 1),
               ending: NSColor(red: 0.25, green: 0.20, blue: 0.66, alpha: 1))!.draw(in: path, angle: -90)

    // the two windows as glass, brighter where they meet (the glyph's own geometry, of 256)
    let g = s * 0.52, o = (s - g) / 2, u = g / 256
    func box(_ x: CGFloat, _ y: CGFloat, _ w: CGFloat, _ alpha: CGFloat) {
        NSColor(white: 1, alpha: alpha).setFill()
        NSRect(x: o + x * u, y: o + (256 - y - w) * u, width: w * u, height: w * u).fill(using: .sourceOver)
    }
    box(40, 40, 120, 0.16)
    box(96, 96, 120, 0.16)
    box(96, 96, 64, 0.45)
    glyph.draw(in: NSRect(x: o, y: o, width: g, height: g))
}

func menuIcon(_ s: CGFloat) {
    glyph.draw(in: NSRect(x: 0, y: 0, width: s, height: s)) // macOS only uses its shape
}

let fm = FileManager.default
let set = "target/AppIcon.iconset"
try? fm.removeItem(atPath: set)
try! fm.createDirectory(atPath: set, withIntermediateDirectories: true)
for (name, px) in [("16x16", 16), ("16x16@2x", 32), ("32x32", 32), ("32x32@2x", 64), ("128x128", 128), ("128x128@2x", 256),
                   ("256x256", 256), ("256x256@2x", 512), ("512x512", 512), ("512x512@2x", 1024)] {
    try! png(px, appIcon).write(to: URL(fileURLWithPath: "\(set)/icon_\(name).png"))
}
try! png(36, menuIcon).write(to: URL(fileURLWithPath: "resources/menubar.png"))
print("icons drawn")
