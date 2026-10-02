import { describe, expect, test } from "bun:test";
import {
	MANAGE,
	NAV,
	type PinnablePlugin,
	PRIMARY,
	pluginPin,
	resolvePins,
	togglePin,
} from "./nav";

const plugin = (id: string): PinnablePlugin => ({ id, title: id });

describe("nav table", () => {
	test("five primary destinations, the rest under Manage", () => {
		expect(PRIMARY).toHaveLength(5);
		expect(PRIMARY.map((n) => n.to)).toEqual([
			"/",
			"/pairing",
			"/displays",
			"/library",
			"/host",
		]);
		expect(PRIMARY.length + MANAGE.length).toBe(NAV.length);
	});

	// "/" matches every path as a prefix, and "/plugins" sits above "/plugins/<id>" — a plugin's
	// own page — so both would light up alongside the page you are actually on.
	test("only the prefix-ambiguous routes are exact", () => {
		expect(NAV.filter((n) => n.exact).map((n) => n.to)).toEqual([
			"/",
			"/plugins",
		]);
	});

	test("every destination carries a phone-list hint", () => {
		for (const n of NAV) expect(n.hint()).not.toBe("");
	});
});

describe("resolvePins", () => {
	test("keeps the operator's order", () => {
		const resolved = resolvePins(
			[pluginPin("virtualhere"), pluginPin("rom-manager")],
			[plugin("rom-manager"), plugin("virtualhere")],
		);
		expect(resolved.map((p) => p.id)).toEqual(["virtualhere", "rom-manager"]);
	});

	// The pin outlives the plugin on purpose: uninstall/reinstall must not silently lose it.
	test("drops a pin nothing resolves, without touching its neighbours", () => {
		const resolved = resolvePins(
			[pluginPin("a"), pluginPin("since-removed"), pluginPin("b")],
			[plugin("a"), plugin("b")],
		);
		expect(resolved.map((p) => p.id)).toEqual(["a", "b"]);
	});

	// A plugin id is not a route. Without the prefix a plugin called "logs" would resolve to
	// Troubleshooting, and a raw route id would match a plugin of the same name.
	test("a plugin id does not resolve as a route", () => {
		expect(resolvePins(["logs"], [plugin("logs")])).toEqual([]);
		expect(resolvePins([pluginPin("logs")], [plugin("logs")])).toHaveLength(1);
	});

	// The sidebar lists every page already; a pinned route only listed it a second time.
	test("a route never resolves", () => {
		expect(resolvePins(["/stats", "/host"], [])).toEqual([]);
	});
});

describe("togglePin", () => {
	test("appends, then removes, leaving order otherwise intact", () => {
		const [a, b] = [pluginPin("a"), pluginPin("b")];
		expect(togglePin([a], b)).toEqual([a, b]);
		expect(togglePin([a, b], a)).toEqual([b]);
	});
});
