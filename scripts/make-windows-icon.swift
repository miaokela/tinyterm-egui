// Renders assets/icon.ico (multi-size, 32-bit) from assets/icon.png.
//
//   swift scripts/make-windows-icon.swift
//
// NSIS needs a real .ico for the installer artwork, and the Windows build embeds
// the same file into TinyTerm.exe via build.rs. Every size is stored as an
// uncompressed BMP/DIB entry rather than a PNG payload, because the NSIS icon
// loader only understands the classic form.
import AppKit
import CoreGraphics

let root = URL(fileURLWithPath: CommandLine.arguments[0]).standardized
    .deletingLastPathComponent().deletingLastPathComponent().path
let srcPath = "\(root)/assets/icon.png"
let outPath = "\(root)/assets/icon.ico"

let sizes = [16, 24, 32, 48, 64, 128, 256]

guard let src = NSImage(contentsOfFile: srcPath),
      let srcCG = src.cgImage(forProposedRect: nil, context: nil, hints: nil) else {
    fatalError("cannot read \(srcPath)")
}

/// Renders one square size and returns BGRA rows top-down plus the alpha channel.
func render(_ size: Int) -> (bgra: [UInt8], alpha: [UInt8]) {
    let bytesPerRow = size * 4
    var buf = [UInt8](repeating: 0, count: bytesPerRow * size)
    buf.withUnsafeMutableBytes { raw in
        let ctx = CGContext(data: raw.baseAddress, width: size, height: size,
                            bitsPerComponent: 8, bytesPerRow: bytesPerRow,
                            space: CGColorSpaceCreateDeviceRGB(),
                            bitmapInfo: CGImageAlphaInfo.premultipliedFirst.rawValue
                                      | CGBitmapInfo.byteOrder32Little.rawValue)!
        ctx.interpolationQuality = .high
        ctx.draw(srcCG, in: CGRect(x: 0, y: 0, width: size, height: size))
    }
    var alpha = [UInt8](repeating: 255, count: size * size)
    for i in 0..<(size * size) { alpha[i] = buf[i * 4 + 3] }
    return (buf, alpha)
}

func le16(_ v: Int) -> [UInt8] { [UInt8(v & 0xff), UInt8((v >> 8) & 0xff)] }
func le32(_ v: Int) -> [UInt8] {
    [UInt8(v & 0xff), UInt8((v >> 8) & 0xff), UInt8((v >> 16) & 0xff), UInt8((v >> 24) & 0xff)]
}

var images: [[UInt8]] = []
for size in sizes {
    let (bgra, alpha) = render(size)
    let maskRow = ((size + 31) / 32) * 4
    var out: [UInt8] = []

    // BITMAPINFOHEADER: height is doubled to cover the XOR bitmap and the mask.
    out += le32(40)
    out += le32(size)
    out += le32(size * 2)
    out += le16(1)          // planes
    out += le16(32)         // bits per pixel
    out += le32(0)          // BI_RGB
    out += le32(size * size * 4 + maskRow * size)
    out += le32(0) + le32(0) + le32(0) + le32(0)

    // XOR bitmap, bottom-up.
    for y in stride(from: size - 1, through: 0, by: -1) {
        out += bgra[(y * size * 4)..<((y + 1) * size * 4)]
    }
    // AND mask, bottom-up, 1 bit per pixel, rows padded to 4 bytes.
    for y in stride(from: size - 1, through: 0, by: -1) {
        var row = [UInt8](repeating: 0, count: maskRow)
        for x in 0..<size where alpha[y * size + x] < 128 {
            row[x / 8] |= UInt8(0x80 >> (x % 8))
        }
        out += row
    }
    images.append(out)
}

var ico: [UInt8] = []
ico += le16(0) + le16(1) + le16(sizes.count)
var offset = 6 + sizes.count * 16
for (i, size) in sizes.enumerated() {
    ico += [UInt8(size == 256 ? 0 : size), UInt8(size == 256 ? 0 : size), 0, 0]
    ico += le16(1) + le16(32)
    ico += le32(images[i].count) + le32(offset)
    offset += images[i].count
}
for img in images { ico += img }

try! Data(ico).write(to: URL(fileURLWithPath: outPath))
print("wrote \(outPath) (\(ico.count) bytes, sizes: \(sizes.map(String.init).joined(separator: ",")))")
