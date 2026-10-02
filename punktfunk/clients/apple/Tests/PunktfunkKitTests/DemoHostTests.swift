// The demo host end to end: `DemoHost` draws and encodes, the core serves it on 127.0.0.1, and a
// real pinned connection receives an H.264 IDR whose in-band SPS/PPS describe the asked size.

import CoreMedia
import XCTest
@testable import PunktfunkKit

final class DemoHostTests: XCTestCase {
    func testAPinnedConnectionReceivesAnIDR() throws {
        let host = try XCTUnwrap(DemoHost(titles: ["custom:aurora": "Aurora Drift"]))
        defer { host.stop() }
        let connection = try PunktfunkConnection(
            host: "127.0.0.1", port: host.port, width: 1280, height: 720, refreshHz: 60,
            pinSHA256: host.fingerprint, videoCodecs: PunktfunkConnection.codecH264,
            launchID: "custom:aurora")
        defer { connection.close() }

        var first: AccessUnit?
        for _ in 0..<50 where first == nil {
            first = try connection.nextAU(timeoutMs: 100)
        }
        let au = try XCTUnwrap(first, "the demo host streams within five seconds")
        let format = try XCTUnwrap(
            AnnexB.formatDescription(fromIDR: au.data, codec: .h264),
            "the first access unit is an IDR carrying SPS/PPS")
        let dims = CMVideoFormatDescriptionGetDimensions(format)
        XCTAssertEqual(Int(dims.width), 1280)
        XCTAssertEqual(Int(dims.height), 720)
    }
}
