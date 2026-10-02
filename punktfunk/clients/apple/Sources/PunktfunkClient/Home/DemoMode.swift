// Demo mode: a saved "Demo Host" served by the in-process `DemoHost`, so App Review can use
// every screen without a PC — the card, its library, a stream and its HUD. Nothing in the UI
// offers it: adding a host at `address` saves it, and the review notes give that address. The
// record keeps a fixed id; the host behind it starts per launch and re-pins the record, since
// its port and certificate are new each run.

import Foundation
import PunktfunkKit

enum DemoMode {
    /// Fixed, so the record survives relaunches and every surface can recognise it.
    static let hostID = UUID(uuidString: "5D3E0000-DE70-4000-8000-00000000DE70")!
    /// Typed into Add Host, saves the demo host. `.punktfunk` is no TLD, so no real host has it.
    static let address = "demo.punktfunk"
    @MainActor private static var server: DemoHost?

    static func isDemo(_ host: StoredHost) -> Bool { host.id == hostID }

    static func isDemoAddress(_ address: String) -> Bool {
        address.caseInsensitiveCompare(Self.address) == .orderedSame
    }

    /// Save or refresh the demo host and start the loopback host behind it. Nil if it didn't start.
    @MainActor @discardableResult
    static func enable(in store: HostStore) -> StoredHost? {
        if server == nil {
            let titles = Dictionary(games.map { ($0.id, $0.title) }, uniquingKeysWith: { a, _ in a })
            server = DemoHost(titles: titles)
        }
        guard let server else { return nil }
        let saved = store.hosts.first(where: isDemo)
        var record = saved ?? StoredHost(id: hostID, name: "Demo Host", address: "127.0.0.1")
        record.address = "127.0.0.1"
        record.port = server.port
        record.pinnedSHA256 = server.fingerprint
        if saved == nil { store.add(record) } else { store.update(record) }
        return record
    }

    /// At launch: bring the demo host back if its record is still saved.
    @MainActor static func resume(in store: HostStore) {
        if store.hosts.contains(where: isDemo) { enable(in: store) }
    }

    /// The record is gone; stop serving it.
    @MainActor static func stop() {
        server?.stop()
        server = nil
    }

    /// The posters `games` point at, drawn in-app.
    static var art: any LibraryArtSource { ShotPosterArt.source }

    /// Four made-up titles. Decoded because `GameEntry`'s memberwise init is internal to the kit.
    static let games: [GameEntry] = {
        // Relative to now, so Recently Played reads the same every run.
        let now = UInt64(Date().timeIntervalSince1970 * 1000)
        let hour: UInt64 = 3_600_000
        let json = """
        [
          {"id": "custom:aurora", "store": "custom", "title": "Aurora Drift",
           "platform": "PS3", "release_year": 2009, "developer": "Nine Lanterns",
           "genres": ["Racing"], "art": {"portrait": "shot://art/aurora"},
           "stats": {"last_played_unix_ms": \(now - 50 * hour), "play_time_ms": 9000000,
                     "last_run_ms": 1800000, "launch_count": 6}},
          {"id": "steam:starfall", "store": "steam", "title": "Starfall Vale",
           "platform": "PC", "release_year": 2024, "developer": "Meridian Foundry",
           "genres": ["Action", "Adventure"],
           "art": {"portrait": "shot://art/starfall"},
           "stats": {"last_played_unix_ms": \(now - 2 * hour), "play_time_ms": 50400000,
                     "last_run_ms": 5400000, "launch_count": 31}},
          {"id": "heroic:neon", "store": "heroic", "title": "Neon Circuit",
           "platform": "PC", "art": {"portrait": "shot://art/neon"},
           "stats": {"last_played_unix_ms": \(now - 21 * 24 * hour), "play_time_ms": 2100000,
                     "last_run_ms": 2100000, "launch_count": 2}},
          {"id": "gog:ember", "store": "gog", "title": "Ember Peaks",
           "art": {"portrait": "shot://art/ember"}}
        ]
        """
        return (try? JSONDecoder().decode([GameEntry].self, from: Data(json.utf8))) ?? []
    }()
}
