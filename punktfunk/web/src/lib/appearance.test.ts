// The appearance cookie is attacker-writable if anything on this origin ever is, and its
// accent reaches a `style` attribute. That is the one line worth a test here.
import { describe, expect, test } from "bun:test";
import {
	ACCENT_SWATCHES,
	appearanceFromHeader,
	DEFAULT_APPEARANCE,
	isSafeColor,
	parseAppearance,
} from "./appearance";

describe("isSafeColor", () => {
	test("takes exactly six hex digits behind a hash", () => {
		expect(isSafeColor("#6c5bf3")).toBe(true);
		expect(isSafeColor("#ABCDEF")).toBe(true);
	});

	// There is no partially-valid colour: anything else is refused rather than repaired,
	// because a repaired value is a guess at what an attacker meant.
	test.each([
		"6c5bf3",
		"#6c5bf",
		"#6c5bf33",
		"#ggghhh",
		"red",
		"var(--x)",
		"#6c5bf3;background:url(x)",
		"</style><script>alert(1)</script>",
		"",
	])("refuses %p", (value) => {
		expect(isSafeColor(value)).toBe(false);
	});
});

describe("parseAppearance", () => {
	test("round-trips a value this console wrote", () => {
		expect(parseAppearance('{"mode":"light","accent":"#6c5bf3"}')).toEqual({
			mode: "light",
			accent: "#6c5bf3",
		});
	});

	test("keeps the valid half when only one field is bad", () => {
		expect(parseAppearance('{"mode":"dark","accent":"javascript:1"}')).toEqual({
			mode: "dark",
			accent: "system",
		});
		expect(parseAppearance('{"mode":"neon","accent":"#c01c28"}')).toEqual({
			mode: DEFAULT_APPEARANCE.mode,
			accent: "#c01c28",
		});
	});

	test.each([undefined, null, "", "not json", "[]", '{"mode":42}'])(
		"falls back to the default on %p",
		(raw) => {
			expect(parseAppearance(raw)).toEqual(DEFAULT_APPEARANCE);
		},
	);
});

describe("appearanceFromHeader", () => {
	test("picks its own cookie out of a header carrying others", () => {
		const header =
			"other=1; pf-appearance=%7B%22mode%22%3A%22light%22%2C%22accent%22%3A%22system%22%7D; last=2";
		expect(appearanceFromHeader(header)).toEqual({
			mode: "light",
			accent: "system",
		});
	});

	test("no cookie is the default, not an error", () => {
		expect(appearanceFromHeader(undefined)).toEqual(DEFAULT_APPEARANCE);
		expect(appearanceFromHeader("other=1")).toEqual(DEFAULT_APPEARANCE);
	});
});

// The swatch row is offered to the operator, so every one of them has to survive its own
// validator — a swatch the console refuses to apply would be a dead control.
test("every shipped swatch is a colour the validator accepts", () => {
	for (const c of ACCENT_SWATCHES) expect(isSafeColor(c)).toBe(true);
});
