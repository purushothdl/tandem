// Draws the app icon and the menu-bar icon. Run: swift scripts/make_icons.swift
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

func appIcon(_ s: CGFloat) {
    let inset = s * 0.06
    let body = NSRect(x: inset, y: inset, width: s - 2 * inset, height: s - 2 * inset)
    let bodyPath = NSBezierPath(roundedRect: body, xRadius: s * 0.2, yRadius: s * 0.2)
    NSGradient(colors: [NSColor(red: 0.07, green: 0.09, blue: 0.16, alpha: 1), NSColor(red: 0.13, green: 0.17, blue: 0.30, alpha: 1)])!
        .draw(in: bodyPath, angle: -60)

    // two tiled windows with a glowing seam
    let pad = s * 0.20, gap = s * 0.045
    let area = NSRect(x: pad, y: pad * 1.05, width: s - 2 * pad, height: s - 2 * pad * 1.05)
    let w = (area.width - gap) / 2
    let left = NSRect(x: area.minX, y: area.minY, width: w * 0.92, height: area.height)
    let right = NSRect(x: area.minX + w * 0.92 + gap, y: area.minY, width: area.width - w * 0.92 - gap, height: area.height)
    for (r, a) in [(left, 0.95), (right, 0.70)] {
        let p = NSBezierPath(roundedRect: r, xRadius: s * 0.035, yRadius: s * 0.035)
        NSColor(white: 1, alpha: a * 0.16).setFill(); p.fill()
        NSColor(white: 1, alpha: a * 0.85).setStroke(); p.lineWidth = s * 0.016; p.stroke()
        // title-bar dots
        for i in 0..<3 {
            let d = NSRect(x: r.minX + s * 0.035 + CGFloat(i) * s * 0.03, y: r.maxY - s * 0.06, width: s * 0.018, height: s * 0.018)
            NSColor(white: 1, alpha: a * 0.7).setFill(); NSBezierPath(ovalIn: d).fill()
        }
    }
    let seamX = left.maxX + gap / 2
    let glow = NSShadow()
    glow.shadowColor = NSColor(red: 0.25, green: 0.85, blue: 1, alpha: 0.95)
    glow.shadowBlurRadius = s * 0.035
    glow.set()
    NSColor(red: 0.35, green: 0.9, blue: 1, alpha: 1).setFill()
    NSBezierPath(roundedRect: NSRect(x: seamX - s * 0.009, y: area.minY - s * 0.02, width: s * 0.018, height: area.height + s * 0.04),
                 xRadius: s * 0.009, yRadius: s * 0.009).fill()
}

func menuIcon(_ s: CGFloat) {
    NSColor.black.setStroke(); NSColor.black.setFill()
    let r = NSRect(x: s * 0.09, y: s * 0.17, width: s * 0.82, height: s * 0.66)
    let p = NSBezierPath(roundedRect: r, xRadius: s * 0.1, yRadius: s * 0.1)
    p.lineWidth = s * 0.09; p.stroke()
    NSBezierPath(roundedRect: NSRect(x: s * 0.455, y: s * 0.17, width: s * 0.09, height: s * 0.66), xRadius: 0, yRadius: 0).fill()
}

let fm = FileManager.default
let set = "resources/AppIcon.iconset"
try? fm.removeItem(atPath: set)
try! fm.createDirectory(atPath: set, withIntermediateDirectories: true)
for (name, px) in [("16x16", 16), ("16x16@2x", 32), ("32x32", 32), ("32x32@2x", 64), ("128x128", 128), ("128x128@2x", 256),
                   ("256x256", 256), ("256x256@2x", 512), ("512x512", 512), ("512x512@2x", 1024)] {
    try! png(px, appIcon).write(to: URL(fileURLWithPath: "\(set)/icon_\(name).png"))
}
try! png(36, menuIcon).write(to: URL(fileURLWithPath: "resources/menubar.png"))
print("icons drawn")
