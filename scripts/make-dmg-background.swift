// Renders the TinyTerm disk-image background (660x440 @1x, 1320x880 @2x).
//
//   swift scripts/make-dmg-background.swift
//
// The layout is deliberate: the band between the drag hint and the two cards is
// left empty because that is where Finder draws the app icon and the
// /Applications alias (icon centres land at y = 165). The bottom 40px is also
// kept clear so nothing is clipped if Finder reserves part of the window for
// its title bar.
//
// CoreGraphics gotcha: drawing a linear gradient with `options: []` corrupts the
// alpha of the *next* radial gradient (it paints a solid blob). Passing
// `.drawsBeforeStartLocation, .drawsAfterEndLocation` to the linear gradient
// avoids it - do not "simplify" that call away.
import AppKit
import CoreGraphics
import CoreText

let W: CGFloat = 660
let H: CGFloat = 440

func col(_ hex: UInt32, _ a: CGFloat = 1) -> CGColor {
    CGColor(srgbRed: CGFloat((hex >> 16) & 0xff)/255, green: CGFloat((hex >> 8) & 0xff)/255,
            blue: CGFloat(hex & 0xff)/255, alpha: a)
}

let titleFont = NSFont.systemFont(ofSize: 18, weight: .semibold)
let subFont   = NSFont.systemFont(ofSize: 11.5, weight: .regular)
let hintFont  = NSFont.systemFont(ofSize: 12, weight: .regular)
let hintBold  = NSFont.systemFont(ofSize: 12, weight: .semibold)
let cardTitle = NSFont.systemFont(ofSize: 11.5, weight: .semibold)
let cardBody  = NSFont.systemFont(ofSize: 11, weight: .regular)
let bodyBold  = NSFont.systemFont(ofSize: 11, weight: .semibold)
let tagFont   = NSFont.systemFont(ofSize: 10, weight: .bold)
let cmdFont   = NSFont(name: "Menlo", size: 10) ?? NSFont.monospacedSystemFont(ofSize: 10, weight: .regular)
let footFont  = NSFont.systemFont(ofSize: 10, weight: .regular)
let logoFont  = NSFont(name: "Menlo-Bold", size: 16) ?? NSFont.monospacedSystemFont(ofSize: 16, weight: .bold)

func attr(_ s: String, _ f: NSFont, _ c: CGColor) -> NSAttributedString {
    let color = NSColor(cgColor: c) ?? .white
    return NSAttributedString(string: s, attributes: [.font: f, .foregroundColor: color])
}
func measure(_ a: NSAttributedString) -> CGSize { a.size() }

/// Draw text with the given top-left position in *top-down* app coordinates.
@discardableResult
func text(_ ctx: CGContext, _ a: NSAttributedString, x: CGFloat, top: CGFloat, align: Align = .left) -> CGSize {
    let sz = measure(a)
    let line = CTLineCreateWithAttributedString(a)
    var asc: CGFloat = 0, desc: CGFloat = 0, lead: CGFloat = 0
    _ = CTLineGetTypographicBounds(line, &asc, &desc, &lead)
    let baselineCG = H - top - asc
    var px = x
    if align == .right { px = x - sz.width }
    if align == .center { px = x - sz.width/2 }
    ctx.textMatrix = .identity
    ctx.textPosition = CGPoint(x: px, y: baselineCG)
    CTLineDraw(line, ctx)
    return sz
}
enum Align { case left, right, center }

func roundedRect(_ ctx: CGContext, _ r: CGRect, _ radius: CGFloat) {
    let p = CGPath(roundedRect: r, cornerWidth: radius, cornerHeight: radius, transform: nil)
    ctx.addPath(p)
}

func render(scale: CGFloat, out: String) {
    let cs = CGColorSpaceCreateDeviceRGB()
    // noneSkipLast keeps the result opaque, which makes the PNG smaller.
    guard let ctx = CGContext(data: nil, width: Int(W*scale), height: Int(H*scale),
                              bitsPerComponent: 8, bytesPerRow: 0, space: cs,
                              bitmapInfo: CGImageAlphaInfo.noneSkipLast.rawValue) else { fatalError("ctx") }
    ctx.scaleBy(x: scale, y: scale)
    ctx.setAllowsAntialiasing(true)
    ctx.interpolationQuality = .high

    // ---- base gradient (160deg: top-left -> bottom-right)
    let base = CGGradient(colorsSpace: cs, colors: [col(0x08172f), col(0x061325), col(0x04101f)] as CFArray,
                          locations: [0, 0.58, 1])!
    ctx.drawLinearGradient(base, start: CGPoint(x: W*0.12, y: H), end: CGPoint(x: W*0.9, y: 0),
                           options: [.drawsBeforeStartLocation, .drawsAfterEndLocation])

    // ---- blue glow top-left
    let glow = CGGradient(colorsSpace: cs, colors: [col(0x0c2c55, 0.85), col(0x0c2c55, 0.0)] as CFArray,
                          locations: [0, 1])!
    ctx.drawRadialGradient(glow, startCenter: CGPoint(x: W*0.18, y: H), startRadius: 0,
                           endCenter: CGPoint(x: W*0.18, y: H), endRadius: W*0.78,
                           options: [.drawsAfterEndLocation])

    // ---- green glow bottom-right
    let glow2 = CGGradient(colorsSpace: cs, colors: [col(0x36e08e, 0.14), col(0x36e08e, 0.0)] as CFArray,
                           locations: [0, 1])!
    ctx.drawRadialGradient(glow2, startCenter: CGPoint(x: W, y: 0), startRadius: 0,
                           endCenter: CGPoint(x: W, y: 0), endRadius: W*0.5,
                           options: [.drawsAfterEndLocation])

    // ---- faint grid
    ctx.saveGState()
    ctx.setStrokeColor(col(0x60a5ff, 0.07 * 0.45))
    ctx.setLineWidth(1)
    var gx: CGFloat = 0
    while gx <= W { ctx.move(to: CGPoint(x: gx, y: 0)); ctx.addLine(to: CGPoint(x: gx, y: H)); gx += 46 }
    var gy: CGFloat = 0
    while gy <= H { ctx.move(to: CGPoint(x: 0, y: gy)); ctx.addLine(to: CGPoint(x: W, y: gy)); gy += 46 }
    ctx.strokePath()
    ctx.restoreGState()

    let padL: CGFloat = 26, padR: CGFloat = 26, padT: CGFloat = 20, padB: CGFloat = 46  // bottom margin keeps the footer safe if Finder clips ~28px of titlebar
    let contentW = W - padL - padR

    // ---- header
    let logoSize: CGFloat = 32
    let logoRect = CGRect(x: padL, y: H - padT - logoSize, width: logoSize, height: logoSize)
    ctx.saveGState()
    roundedRect(ctx, logoRect, 9)
    ctx.clip()
    let lg = CGGradient(colorsSpace: cs, colors: [col(0x78beff, 0.55), col(0x2f7dff, 0.18)] as CFArray,
                        locations: [0, 1])!
    ctx.drawRadialGradient(lg, startCenter: CGPoint(x: logoRect.minX + 9, y: logoRect.maxY - 8), startRadius: 0,
                           endCenter: CGPoint(x: logoRect.minX + 9, y: logoRect.maxY - 8), endRadius: logoSize,
                           options: [.drawsAfterEndLocation])
    ctx.restoreGState()
    ctx.setStrokeColor(col(0x70bfff, 0.45)); ctx.setLineWidth(1)
    roundedRect(ctx, logoRect.insetBy(dx: 0.5, dy: 0.5), 9); ctx.strokePath()
    let logoText = attr(">_", logoFont, col(0x8ec5ff))
    let logoTextSize = measure(logoText)
    text(ctx, logoText, x: logoRect.midX - logoTextSize.width/2, top: padT + (logoSize - logoTextSize.height)/2)

    // ---- title (vertically centered on the logo)
    let titleAttr = attr("安装 TinyTerm", titleFont, col(0xe7eff9))
    let titleSize = measure(titleAttr)
    text(ctx, titleAttr, x: logoRect.maxX + 10, top: padT + (logoSize - titleSize.height)/2)
    // ---- sub, right aligned
    let subAttr = attr("macOS 通用版 · arm64 + x86_64", subFont, col(0x8ea6bd))
    let subSize = measure(subAttr)
    text(ctx, subAttr, x: W - padR, top: padT + (logoSize - subSize.height)/2, align: .right)

    // ---- hint
    let hintTop = padT + logoSize + 8
    let hintParts = NSMutableAttributedString()
    hintParts.append(attr("把 ", hintFont, col(0x9bb2c8)))
    hintParts.append(attr("TinyTerm", hintBold, col(0x8ec5ff)))
    hintParts.append(attr(" 拖到右侧的 ", hintFont, col(0x9bb2c8)))
    hintParts.append(attr("Applications", hintBold, col(0x8ec5ff)))
    hintParts.append(attr(" 文件夹即可完成安装", hintFont, col(0x9bb2c8)))
    let hintSize = text(ctx, hintParts, x: padL, top: hintTop)

    // ================= cards =================
    let cardGap: CGFloat = 10
    let cardW = (contentW - cardGap) / 2
    let cardPadX: CGFloat = 12, cardPadT: CGFloat = 10, cardPadB: CGFloat = 11
    let innerW = cardW - cardPadX*2

    struct Line { let s: NSAttributedString; let h: CGFloat }
    var cardLines: [[Line]] = []

    // --- card 1
    var c1: [Line] = []
    let tag1 = attr("情况一", tagFont, col(0xff8f8f))
    let t1 = attr("提示「\"TinyTerm\"已损坏」", cardTitle, col(0xdfe9f5))
    let b1 = attr("打开「终端」，粘贴并回车：", cardBody, col(0x9bb2c8))
    let cmdAttr = attr("xattr -cr /Applications/TinyTerm.app", cmdFont, col(0x8be0e8))
    let cmdSize = measure(cmdAttr)
    let cmdBoxH = cmdSize.height + 10
    c1.append(Line(s: tag1, h: measure(tag1).height + 2))
    c1.append(Line(s: t1, h: measure(t1).height))
    c1.append(Line(s: b1, h: measure(b1).height))
    c1.append(Line(s: cmdAttr, h: cmdBoxH))
    var h1 = cardPadT + cardPadB
    h1 += c1[0].h + 6 + c1[1].h + 6 + c1[2].h + 6 + c1[3].h

    // --- card 2
    var c2: [Line] = []
    let tag2 = attr("情况二", tagFont, col(0xf5b871))
    let t2 = attr("提示「无法验证开发者」", cardTitle, col(0xdfe9f5))
    let s1 = NSMutableAttributedString()
    s1.append(attr("1. ", cardBody, col(0x8ec5ff))); s1.append(attr("点「完成」关闭弹窗", cardBody, col(0x9bb2c8)))
    let s2 = NSMutableAttributedString()
    s2.append(attr("2. ", cardBody, col(0x8ec5ff))); s2.append(attr("系统设置 → ", cardBody, col(0x9bb2c8)))
    s2.append(attr("隐私与安全性", bodyBold, col(0xdfe9f5)))
    let s3 = NSMutableAttributedString()
    s3.append(attr("3. ", cardBody, col(0x8ec5ff))); s3.append(attr("找到 TinyTerm，点「", cardBody, col(0x9bb2c8)))
    s3.append(attr("仍要打开", bodyBold, col(0xdfe9f5))); s3.append(attr("」", cardBody, col(0x9bb2c8)))
    let stepH = measure(s1).height * 1.35
    c2.append(Line(s: tag2, h: measure(tag2).height + 2))
    c2.append(Line(s: t2, h: measure(t2).height))
    c2.append(Line(s: s1, h: stepH))
    c2.append(Line(s: s2, h: stepH))
    c2.append(Line(s: s3, h: stepH))
    var h2 = cardPadT + cardPadB
    h2 += c2[0].h + 6 + c2[1].h + 6 + c2[2].h + c2[3].h + c2[4].h
    cardLines = [c1, c2]

    // ---- footer
    let footAttr = attr("本应用开源免费、未做 Apple 签名与公证，以上为 macOS 的常规绕过方式", footFont, col(0x6f859b))
    let footSize = measure(footAttr)
    let footTop = H - padB - footSize.height

    let cardH = max(h1, h2)
    let cardsBottom = footTop - 8
    let cardsTop = cardsBottom - cardH

    let bounds = [col(0x3a84ff, 0.28), col(0xe0575c, 0.42), col(0xf0a040, 0.42)]
    let tagBG = [col(0x000000, 0), col(0xe0575c, 0.16), col(0xf0a040, 0.16)]

    for i in 0..<2 {
        let cx = padL + CGFloat(i) * (cardW + cardGap)
        let cardRect = CGRect(x: cx, y: H - cardsTop - cardH, width: cardW, height: cardH)
        // fill + border
        ctx.saveGState()
        roundedRect(ctx, cardRect, 10)
        ctx.setFillColor(col(0x091628, 0.80)); ctx.fillPath()
        roundedRect(ctx, cardRect.insetBy(dx: 0.5, dy: 0.5), 10)
        ctx.setStrokeColor(bounds[i == 0 ? 1 : 2]); ctx.setLineWidth(1); ctx.strokePath()
        ctx.restoreGState()

        var y = cardsTop + cardPadT
        let lines = cardLines[i]
        for (li, ln) in lines.enumerated() {
            let sz = measure(ln.s)
            if li == 0 {  // tag pill
                let pillW = sz.width + 14, pillH = ln.h
                let pill = CGRect(x: cx + cardPadX, y: H - y - pillH, width: pillW, height: pillH)
                ctx.saveGState(); roundedRect(ctx, pill, 6)
                ctx.setFillColor(tagBG[i == 0 ? 1 : 2]); ctx.fillPath(); ctx.restoreGState()
                text(ctx, ln.s, x: pill.minX + 7, top: y + 1)
                y += ln.h + 6
            } else if li == 3 && i == 0 {  // command box
                let box = CGRect(x: cx + cardPadX, y: H - y - ln.h, width: innerW, height: ln.h)
                ctx.saveGState(); roundedRect(ctx, box, 6)
                ctx.setFillColor(col(0x050b14, 0.85)); ctx.fillPath()
                roundedRect(ctx, box.insetBy(dx: 0.5, dy: 0.5), 6)
                ctx.setStrokeColor(col(0x3a84ff, 0.22)); ctx.setLineWidth(1); ctx.strokePath(); ctx.restoreGState()
                text(ctx, ln.s, x: box.minX + 8, top: y + (ln.h - sz.height)/2)
                y += ln.h
            } else {
                text(ctx, ln.s, x: cx + cardPadX, top: y)
                y += ln.h
            }
        }
    }
    text(ctx, footAttr, x: W/2, top: footTop, align: .center)

    guard let img = ctx.makeImage() else { fatalError("image") }
    let rep = NSBitmapImageRep(cgImage: img)
    guard let data = rep.representation(using: .png, properties: [:]) else { fatalError("png") }
    try! data.write(to: URL(fileURLWithPath: out))

    // geometry report (for verification)
    print("scale=\(scale) size=\(Int(W*scale))x\(Int(H*scale))")
    print("  header bottom = \(padT + logoSize)")
    print("  hint    top=\(hintTop) bottom=\(hintTop + hintSize.height)")
    print("  cards   top=\(cardsTop) bottom=\(cardsBottom) h=\(cardH) (h1=\(h1) h2=\(h2))")
    print("  foot    top=\(footTop) bottom=\(footTop + footSize.height)  (canvas H=\(H))")
    print("  empty band for icons: \(hintTop + hintSize.height) .. \(cardsTop)  center=\((hintTop + hintSize.height + cardsTop)/2)")
    print("  card inner width=\(innerW) cmd width=\(cmdSize.width) fits=\(cmdSize.width <= innerW - 16)")
}

// assets/ sits next to the scripts/ directory holding this file.
let root = URL(fileURLWithPath: CommandLine.arguments[0]).standardized
    .deletingLastPathComponent().deletingLastPathComponent().path
render(scale: 2, out: "\(root)/assets/dmg-background@2x.png")
render(scale: 1, out: "\(root)/assets/dmg-background.png")
