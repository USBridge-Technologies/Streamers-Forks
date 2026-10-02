// The operator's own theme choice, which outranks whatever the desktop reports
// (design/web-console-overhaul.md D7, §7.2).
//
// A COOKIE, not `localStorage`, for one reason: `__root.tsx` reads it during the server render,
// so the first paint is already right. A value read after hydration means a flash of the wrong
// theme on every page load, which is exactly the thing an appearance setting exists to avoid.
//
// Per browser by design — a phone in dark and a desk in light is the expected shape. If "the
// same override everywhere" is ever asked for, a host-side prefs file is the upgrade and this
// becomes its cache.
export const APPEARANCE_COOKIE = "pf-appearance";

export interface Appearance {
	/** `system` follows the host's own theme; the other two override it. */
	mode: "system" | "light" | "dark";
	/** `system`, or a `#rrggbb` the operator picked. */
	accent: string;
}

export const DEFAULT_APPEARANCE: Appearance = {
	mode: "system",
	accent: "system",
};

/**
 * The console's own swatches: the brand violet plus seven picked to clear the contrast table
 * `check-omarchy-palette.mjs` enforces, in both modes.
 */
export const ACCENT_SWATCHES = [
	"#6c5bf3", // punktfunk violet
	"#3584e4", // libadwaita blue
	"#2ec27e", // green
	"#c88800", // amber
	"#e66100", // orange
	"#c01c28", // red
	"#c061cb", // purple
	"#6f8396", // slate
] as const;

/**
 * The one security line here: this value reaches a `style` attribute. Anything that is not
 * exactly six hex digits is refused rather than sanitised — there is no partially-valid colour,
 * and a cookie is attacker-writable if anything on this origin ever is.
 */
export function isSafeColor(value: string): boolean {
	return /^#[0-9a-fA-F]{6}$/.test(value);
}

/** Parse a cookie value, falling back to the default on anything unexpected. */
export function parseAppearance(raw: string | undefined | null): Appearance {
	if (!raw) return DEFAULT_APPEARANCE;
	try {
		const v = JSON.parse(decodeURIComponent(raw)) as Partial<Appearance>;
		const mode =
			v.mode === "light" || v.mode === "dark" || v.mode === "system"
				? v.mode
				: DEFAULT_APPEARANCE.mode;
		const accent =
			v.accent === "system" ||
			(typeof v.accent === "string" && isSafeColor(v.accent))
				? v.accent
				: DEFAULT_APPEARANCE.accent;
		return { mode, accent };
	} catch {
		return DEFAULT_APPEARANCE;
	}
}

/** Read the cookie from a raw `Cookie:` header — the server render's only source. */
export function appearanceFromHeader(
	header: string | undefined | null,
): Appearance {
	if (!header) return DEFAULT_APPEARANCE;
	for (const part of header.split(";")) {
		const [name, ...rest] = part.trim().split("=");
		if (name === APPEARANCE_COOKIE) return parseAppearance(rest.join("="));
	}
	return DEFAULT_APPEARANCE;
}

/** Read it in the browser. */
export function readAppearance(): Appearance {
	if (typeof document === "undefined") return DEFAULT_APPEARANCE;
	return appearanceFromHeader(document.cookie);
}

/**
 * Write it, and let the caller decide what to do next. A year, `SameSite=Lax`, path-wide:
 * this is a display preference, not a credential, and it has to survive a normal navigation.
 */
export function writeAppearance(next: Appearance): void {
	if (typeof document === "undefined") return;
	const value = encodeURIComponent(JSON.stringify(next));
	// biome-ignore lint/suspicious/noDocumentCookie: the Cookie Store API is not in Safari, and this console is opened from couch devices as often as from a desk.
	document.cookie = `${APPEARANCE_COOKIE}=${value}; Path=/; Max-Age=31536000; SameSite=Lax`;
}
