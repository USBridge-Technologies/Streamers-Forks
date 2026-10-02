// The ring's stick aiming (design/touch-client-overlay.md §2.6): the thumb points at a slot
// rather than stepping one disc per push. Pinned here because the geometry is duplicated in
// three languages — `pf_client_core::menu_nav::ring_sector` is the reference, and a drift in
// the +y sense or the 12-o'clock origin turns "aim at Disconnect" into "aim at Keyboard".

import GameController
import XCTest

@testable import PunktfunkKit

final class RingSectorTests: XCTestCase {
    /// GameController is +y = up; slot k sits at `-90° + 60°·k` on screen, 12 o'clock first,
    /// going clockwise. So up is 0, down is 3, and 2 o'clock (up and right) is 1.
    func testTheThumbPointsAtTheSlotUnderIt() {
        XCTAssertEqual(GamepadCapture.ringSector(0, 1, nil), 0)
        XCTAssertEqual(GamepadCapture.ringSector(0, -1, nil), 3)
        XCTAssertEqual(GamepadCapture.ringSector(0.866, 0.5, nil), 1)
        XCTAssertEqual(GamepadCapture.ringSector(0.866, -0.5, nil), 2)
        XCTAssertEqual(GamepadCapture.ringSector(-0.866, -0.5, nil), 4)
        XCTAssertEqual(GamepadCapture.ringSector(-0.866, 0.5, nil), 5)
    }

    /// A resting thumb owns no slot: the ring goes back to its centre, and drift never aims.
    func testNeutralOwnsNothing() {
        XCTAssertNil(GamepadCapture.ringSector(0, 0, nil))
        XCTAssertNil(GamepadCapture.ringSector(0.4, 0.2, nil))
        // A diagonal counts by magnitude — 0.4/0.4 is past 0.5 out, though neither axis is.
        XCTAssertEqual(GamepadCapture.ringSector(0.4, 0.4, nil), 1)
    }

    /// The engaged sector holds past its 30° edge, so a thumb parked on a boundary cannot
    /// flicker between two discs; the looser release floor keeps it engaged as the stick eases.
    func testAnEngagedSectorHoldsTheBoundary() {
        // 32° past slot 0's centre: slot 1's half by angle, still slot 0 once engaged.
        let x = Float(sin(32 * Double.pi / 180)), y = Float(cos(32 * Double.pi / 180))
        XCTAssertEqual(GamepadCapture.ringSector(x, y, nil), 1)
        XCTAssertEqual(GamepadCapture.ringSector(x, y, 0), 0)
        // 40° past is nobody's boundary case — the overlap is 5°.
        let fx = Float(sin(40 * Double.pi / 180)), fy = Float(cos(40 * Double.pi / 180))
        XCTAssertEqual(GamepadCapture.ringSector(fx, fy, 0), 1)
        // Eased back to 0.4 out: too weak to engage, strong enough to keep what it had.
        XCTAssertNil(GamepadCapture.ringSector(0, 0.4, nil))
        XCTAssertEqual(GamepadCapture.ringSector(0, 0.4, 0), 0)
    }
}
