// Draws an input-menu template icon (black glyph on transparency, 16 pt at 1x and 2x) into one TIFF.
//   swift Tools/make-icon.swift <output.tiff> [glyph] [font size]
import AppKit

let output = CommandLine.arguments.count > 1 ? CommandLine.arguments[1] : "icon.tiff"
let glyph = CommandLine.arguments.count > 2 ? CommandLine.arguments[2] : "한"
let fontSize = CommandLine.arguments.count > 3 ? CGFloat(Double(CommandLine.arguments[3]) ?? 13) : 13
let points: CGFloat = 16

func render(scale: CGFloat) -> NSBitmapImageRep {
    let pixels = Int(points * scale)
    let rep = NSBitmapImageRep(bitmapDataPlanes: nil, pixelsWide: pixels, pixelsHigh: pixels, bitsPerSample: 8,
                               samplesPerPixel: 4, hasAlpha: true, isPlanar: false, colorSpaceName: .deviceRGB,
                               bytesPerRow: 0, bitsPerPixel: 0)!
    rep.size = NSSize(width: points, height: points)
    NSGraphicsContext.saveGraphicsState()
    NSGraphicsContext.current = NSGraphicsContext(bitmapImageRep: rep)
    let font = NSFont.systemFont(ofSize: fontSize, weight: .semibold)
    let text = NSAttributedString(string: glyph, attributes: [.font: font, .foregroundColor: NSColor.black])
    let size = text.size()
    text.draw(at: NSPoint(x: (points - size.width) / 2, y: (points - size.height) / 2))
    NSGraphicsContext.restoreGraphicsState()
    return rep
}

let image = NSImage(size: NSSize(width: points, height: points))
image.addRepresentation(render(scale: 1))
image.addRepresentation(render(scale: 2))
guard let tiff = image.tiffRepresentation(using: .lzw, factor: 0) else { fatalError("TIFF encoding failed") }
try tiff.write(to: URL(fileURLWithPath: output))
print("icon \(output)")
