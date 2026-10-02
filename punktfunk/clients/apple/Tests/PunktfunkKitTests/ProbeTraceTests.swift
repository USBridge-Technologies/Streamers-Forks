import XCTest
@testable import PunktfunkKit

final class ProbeTraceTests: XCTestCase {
    private func partial(_ ms: UInt32, _ bytes: UInt64) -> PunktfunkConnection.ProbeResult {
        .init(
            done: false, recvBytes: bytes, recvPackets: 0, hostBytes: bytes, hostPackets: 0,
            elapsedMs: ms, throughputKbps: 0, lossPct: 0)
    }

    func testEachPollBecomesTheRateOfItsOwnSlice() {
        var trace = PunktfunkConnection.ProbeTrace()
        trace.add(partial(200, 25_000_000)) // 1 Gbps over the first 200 ms
        trace.add(partial(400, 37_500_000)) // 500 Mbps over the next 200 ms
        XCTAssertEqual(trace.points.map(\.seconds), [0.2, 0.4])
        XCTAssertEqual(trace.points[0].mbps, 1000, accuracy: 0.001)
        XCTAssertEqual(trace.points[1].mbps, 500, accuracy: 0.001)
    }

    func testAPollThatDoesNotMoveTheClockAddsNothing() {
        var trace = PunktfunkConnection.ProbeTrace()
        trace.add(partial(0, 0)) // polled before the first probe arrived
        trace.add(partial(200, 12_500_000))
        trace.add(partial(200, 12_500_000))
        XCTAssertEqual(trace.points.count, 1)
        XCTAssertEqual(trace.points[0].mbps, 500, accuracy: 0.001)
    }
}
