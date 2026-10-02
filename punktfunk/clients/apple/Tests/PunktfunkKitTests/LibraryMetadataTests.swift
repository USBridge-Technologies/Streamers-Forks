// The metadata the title sheet shows, and art a plugin inlined as a `data:` URL.

import XCTest
@testable import PunktfunkKit

final class LibraryMetadataTests: XCTestCase {
    func testDecodesDescriptionTagsAndPlayers() throws {
        let json = Data("""
        [{"id":"rom-manager:1","store":"rom-manager","title":"Mario Kart","art":{},
          "description":"Race.","tags":["co-op"],"players":4,"release_year":1992}]
        """.utf8)
        let game = try XCTUnwrap(try JSONDecoder().decode([GameEntry].self, from: json).first)
        XCTAssertEqual(game.description, "Race.")
        XCTAssertEqual(game.tags, ["co-op"])
        XCTAssertEqual(game.players, 4)
        XCTAssertEqual(game.releaseYear, 1992)
    }

    func testAnOlderHostOmitsThemAll() throws {
        let json = Data(#"[{"id":"steam:1","store":"steam","title":"T","art":{}}]"#.utf8)
        let game = try XCTUnwrap(try JSONDecoder().decode([GameEntry].self, from: json).first)
        XCTAssertNil(game.description)
        XCTAssertNil(game.tags)
        XCTAssertNil(game.players)
    }

    func testInlineArtDecodesBase64AndRefusesTheRest() throws {
        let png = Data([0x89, 0x50, 0x4E, 0x47])
        let url = try XCTUnwrap(URL(string: "data:image/png;base64,\(png.base64EncodedString())"))
        XCTAssertEqual(try LibraryArtLoader.inlineBytes(url), png)
        let plain = try XCTUnwrap(URL(string: "data:text/plain,hello"))
        XCTAssertThrowsError(try LibraryArtLoader.inlineBytes(plain))
    }
}
