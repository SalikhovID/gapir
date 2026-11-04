#!/usr/bin/env swift
// Renders the Gapir app icon and menu bar icons. Run from the repo root:
//   swift scripts/make-icons.swift && npx @tauri-apps/cli@2 icon src-tauri/icons/app-icon.png -o src-tauri/icons
import AppKit

func png(_ size: Int, _ path: String, _ draw: (CGContext, CGFloat) -> Void) {
    let rep = NSBitmapImageRep(
        bitmapDataPlanes: nil, pixelsWide: size, pixelsHigh: size, bitsPerSample: 8,
        samplesPerPixel: 4, hasAlpha: true, isPlanar: false, colorSpaceName: .deviceRGB,
        bytesPerRow: 0, bitsPerPixel: 0)!
    NSGraphicsContext.saveGraphicsState()
    let context = NSGraphicsContext(bitmapImageRep: rep)!
    NSGraphicsContext.current = context
    draw(context.cgContext, CGFloat(size))
    NSGraphicsContext.restoreGraphicsState()
    try! rep.representation(using: .png, properties: [:])!.write(to: URL(fileURLWithPath: path))
}

/// Five rounded bars of a voice waveform, centred in a square of side `side`.
func bars(_ c: CGContext, side: CGFloat, color: CGColor) {
    let heights: [CGFloat] = [0.26, 0.5, 0.74, 0.42, 0.2]
    let width = side * 0.095
    let gap = side * 0.07
    let total = CGFloat(heights.count) * width + CGFloat(heights.count - 1) * gap
    var x = (side - total) / 2
    c.setFillColor(color)
    for height in heights {
        let h = side * height
        let rect = CGRect(x: x, y: (side - h) / 2, width: width, height: h)
        c.addPath(CGPath(roundedRect: rect, cornerWidth: width / 2, cornerHeight: width / 2, transform: nil))
        c.fillPath()
        x += width + gap
    }
}

let dir = "src-tauri/icons/"

png(1024, dir + "app-icon.png") { c, side in
    let inset = side * 0.09
    let body = CGRect(x: inset, y: inset, width: side - 2 * inset, height: side - 2 * inset)
    let radius = body.width * 0.225
    c.saveGState()
    c.addPath(CGPath(roundedRect: body, cornerWidth: radius, cornerHeight: radius, transform: nil))
    c.clip()
    let colors = [
        CGColor(red: 0.05, green: 0.46, blue: 0.43, alpha: 1),
        CGColor(red: 0.04, green: 0.22, blue: 0.36, alpha: 1),
    ] as CFArray
    let gradient = CGGradient(colorsSpace: CGColorSpaceCreateDeviceRGB(), colors: colors, locations: [0, 1])!
    c.drawLinearGradient(gradient, start: CGPoint(x: 0, y: side), end: CGPoint(x: side, y: 0), options: [])
    c.restoreGState()
    let scale: CGFloat = 0.6
    c.translateBy(x: side * (1 - scale) / 2, y: side * (1 - scale) / 2)
    bars(c, side: side * scale, color: CGColor(red: 0.98, green: 0.96, blue: 0.90, alpha: 1))
}

// Template image: macOS tints it for light and dark menu bars.
png(44, dir + "tray-idle.png") { c, side in
    bars(c, side: side, color: CGColor(gray: 0, alpha: 1))
}

png(44, dir + "tray-recording.png") { c, side in
    let margin = side * 0.08
    c.setFillColor(CGColor(red: 0.92, green: 0.20, blue: 0.20, alpha: 1))
    c.fillEllipse(in: CGRect(x: margin, y: margin, width: side - 2 * margin, height: side - 2 * margin))
    bars(c, side: side, color: CGColor(gray: 1, alpha: 1))
}

png(44, dir + "tray-busy.png") { c, side in
    let radius = side * 0.08
    c.setFillColor(CGColor(gray: 0, alpha: 1))
    for offset in [-0.26, 0, 0.26] as [CGFloat] {
        let x = side * (0.5 + offset) - radius
        c.fillEllipse(in: CGRect(x: x, y: side / 2 - radius, width: radius * 2, height: radius * 2))
    }
}
