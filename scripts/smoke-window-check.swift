// Finds the frontmost window owned by "Oikonomia", captures it, and exits
// nonzero when the capture is (near-)uniformly white - the blank-webview
// failure the bare binary always shows and a broken bundle can show.
//
// Adaptation note: the original approach called CGWindowListCreateImage
// directly, but that API is obsoleted on macOS 15+ ("Please use
// ScreenCaptureKit instead"), so on this machine it fails to even compile.
// Rather than pull in the async ScreenCaptureKit APIs, this shells out to
// the `screencapture` CLI (`-l <windowid>`), which still performs a
// synchronous single-window capture and is gated by the same Screen
// Recording permission. The behavior contract is unchanged: find the
// Oikonomia window (>300px wide), capture it, sample pixels on an 8px
// stride, and fail below a 5% non-near-white ratio.
import CoreGraphics
import Foundation
import ImageIO

func fail(_ message: String) -> Never {
    FileHandle.standardError.write((message + "\n").data(using: .utf8)!)
    exit(1)
}

let windowList = CGWindowListCopyWindowInfo([.optionOnScreenOnly], kCGNullWindowID)
    as? [[String: Any]] ?? []
guard let window = windowList.first(where: {
    ($0[kCGWindowOwnerName as String] as? String) == "Oikonomia"
        && (($0[kCGWindowBounds as String] as? [String: Any])?["Width"] as? Double ?? 0) > 300
}), let windowID = window[kCGWindowNumber as String] as? CGWindowID else {
    fail("no Oikonomia window found")
}

let tempURL = URL(fileURLWithPath: NSTemporaryDirectory())
    .appendingPathComponent("oikonomia-smoke-\(ProcessInfo.processInfo.globallyUniqueString).png")
defer { try? FileManager.default.removeItem(at: tempURL) }

let capture = Process()
capture.executableURL = URL(fileURLWithPath: "/usr/sbin/screencapture")
capture.arguments = ["-x", "-o", "-l", "\(windowID)", tempURL.path]
do {
    try capture.run()
} catch {
    fail("could not launch screencapture: \(error)")
}
capture.waitUntilExit()
guard capture.terminationStatus == 0,
      FileManager.default.fileExists(atPath: tempURL.path) else {
    fail("could not capture window (grant Screen Recording to the terminal)")
}

guard let source = CGImageSourceCreateWithURL(tempURL as CFURL, nil),
      let image = CGImageSourceCreateImageAtIndex(source, 0, nil) else {
    fail("could not decode captured window image")
}

let width = image.width
let height = image.height
guard width > 0, height > 0 else {
    fail("captured image has zero size")
}

let bytesPerPixel = 4
let bytesPerRow = width * bytesPerPixel
var pixels = [UInt8](repeating: 0, count: height * bytesPerRow)
guard let colorSpace = CGColorSpace(name: CGColorSpace.sRGB),
      let context = CGContext(
        data: &pixels,
        width: width,
        height: height,
        bitsPerComponent: 8,
        bytesPerRow: bytesPerRow,
        space: colorSpace,
        bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue
      ) else {
    fail("could not create bitmap context")
}
context.draw(image, in: CGRect(x: 0, y: 0, width: width, height: height))

var nonWhite = 0
var total = 0
for y in stride(from: 0, to: height, by: 8) {
    for x in stride(from: 0, to: width, by: 8) {
        let offset = y * bytesPerRow + x * bytesPerPixel
        let r = pixels[offset], g = pixels[offset + 1], b = pixels[offset + 2]
        total += 1
        if r < 240 || g < 240 || b < 240 { nonWhite += 1 }
    }
}
let ratio = total == 0 ? 0 : Double(nonWhite) / Double(total)
print("non-white pixel ratio: \(ratio)")
if ratio < 0.05 {
    fail("window is (near-)blank - the UI did not render")
}
