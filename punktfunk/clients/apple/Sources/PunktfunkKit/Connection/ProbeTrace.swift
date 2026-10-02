// The speed test's goodput over time. The core reports cumulative counters, so each poll's slice
// (bytes and milliseconds since the previous poll) becomes one point on the page's chart.

import Foundation

extension PunktfunkConnection {
    /// Goodput per poll of `probeResult()`, for charting a burst as it lands.
    public struct ProbeTrace: Sendable, Equatable {
        public struct Point: Sendable, Equatable {
            /// Seconds into the receive window, at the end of this slice.
            public let seconds: Double
            public let mbps: Double
        }

        public private(set) var points: [Point] = []
        private var lastMs: UInt32 = 0
        private var lastBytes: UInt64 = 0

        public init() {}

        /// Adds the slice since the previous result. One that does not move the clock adds nothing.
        public mutating func add(_ result: ProbeResult) {
            guard result.elapsedMs > lastMs, result.recvBytes >= lastBytes else { return }
            let bits = Double(result.recvBytes - lastBytes) * 8
            let ms = Double(result.elapsedMs - lastMs)
            points.append(Point(seconds: Double(result.elapsedMs) / 1000, mbps: bits / ms / 1000))
            lastMs = result.elapsedMs
            lastBytes = result.recvBytes
        }
    }
}
