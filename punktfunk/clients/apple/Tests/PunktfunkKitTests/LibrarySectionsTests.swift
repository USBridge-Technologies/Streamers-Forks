import XCTest

@testable import PunktfunkKit

/// The stored-layout rules for the Library tab's sections: order, switches, and what an older
/// or newer build's value turns into.
final class LibrarySectionsTests: XCTestCase {
    func testNothingStoredIsEverySectionInOrder() {
        let all: [LibrarySection] = [.desktops, .recent, .favorites, .launchers, .games]
        XCTAssertEqual(LibrarySectionLayout(stored: nil).visible, all)
        XCTAssertEqual(LibrarySectionLayout(stored: "").visible, all)
    }

    func testOrderAndSwitchesRoundTrip() {
        let stored = "games,-launchers,favorites,desktops,-recent"
        let layout = LibrarySectionLayout(stored: stored)
        XCTAssertEqual(layout.stored, stored)
        XCTAssertEqual(layout.visible, [.games, .favorites, .desktops])
    }

    func testUnknownIdsDropAndMissingSectionsAppendSwitchedOn() {
        let layout = LibrarySectionLayout(stored: "games,-something-newer,recent")
        XCTAssertEqual(
            layout.entries.map(\.section), [.games, .recent, .desktops, .favorites, .launchers])
        XCTAssertEqual(layout.stored, "games,recent,desktops,favorites,launchers")
    }

    func testARepeatedIdKeepsItsFirstPlace() {
        let layout = LibrarySectionLayout(stored: "-games,recent,games")
        XCTAssertEqual(layout.entries.first, .init(section: .games, isOn: false))
        XCTAssertEqual(layout.entries.filter { $0.section == .games }.count, 1)
    }
}
