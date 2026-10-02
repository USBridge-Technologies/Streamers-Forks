// The device credential, end to end against a stub host — and the one thing that must never
// drift: the bytes a signature covers, which the real host verifies with
// `punktfunk_core::quic::auth_signed_message`.

import { describe, expect, it } from "bun:test";
import {
	DEVICE_AUTH_CONTEXT,
	DeviceRefused,
	deviceKey,
	type Signer,
	signedMessage,
	staticBearer,
} from "../src/credential.js";
import { derToRaw, fromBase64, rawToDer } from "../src/ecdsa.js";
import { httpRequest } from "../src/http.js";
import { connection, type Fetch } from "../src/connection.js";

/** A signer that records what it was asked to sign and returns a recognisable raw signature. */
const recordingSigner = (): Signer & { signed: Uint8Array[] } => {
	const signed: Uint8Array[] = [];
	return {
		signed,
		spki: () => Promise.resolve("c3BraQ=="), // "spki"
		sign: (m) => {
			signed.push(m);
			return Promise.resolve(new Uint8Array(64).fill(0x11));
		},
	};
};

/**
 * A host that hands out nonces, accepts exactly one exchange per nonce, and can be told to
 * start refusing tokens — which is what a restart looks like from the outside.
 */
const stubHost = () => {
	const nonces = new Set<string>();
	let issued = 0;
	let live = new Set<string>();
	let calls: string[] = [];
	const fetchImpl: Fetch = async (input, init) => {
		const url = String(input);
		const method = init?.method ?? "GET";
		calls.push(`${method} ${new URL(url).pathname}`);
		if (url.endsWith("/auth/device/challenge")) {
			const nonce = (issued++).toString(16).padStart(64, "0");
			nonces.add(nonce);
			return Response.json({ nonce, expires_in: 60 });
		}
		if (url.endsWith("/auth/device/token")) {
			const body = JSON.parse(String(init?.body)) as {
				nonce: string;
				signature: string;
				device_key: string;
			};
			if (!nonces.delete(body.nonce)) return Response.json({ error: "no" }, { status: 401 });
			// The host takes DER; a raw signature must have been converted on the way here.
			expect(() => derToRaw(fromBase64(body.signature))).not.toThrow();
			const token = `tok-${issued}`;
			live.add(token);
			return Response.json({ token, expires_at: Math.floor(Date.now() / 1000) + 3600, fingerprint: "fp" });
		}
		const auth = (init?.headers as Record<string, string>)?.authorization ?? "";
		const token = auth.replace(/^Bearer /, "");
		if (!live.has(token)) return Response.json({ error: "unauthorized" }, { status: 401 });
		return Response.json({ ok: true, path: new URL(url).pathname });
	};
	return {
		fetch: fetchImpl,
		/** Forget every token, as a restarted host does. */
		restart: () => {
			live = new Set();
		},
		calls: () => calls,
		reset: () => {
			calls = [];
		},
	};
};

describe("signedMessage", () => {
	it("is context, then the 32-byte binding, then the 32-byte nonce", () => {
		const m = signedMessage("aa".repeat(32), "bb".repeat(32));
		const ctx = new TextEncoder().encode(DEVICE_AUTH_CONTEXT);
		expect(m.length).toBe(ctx.length + 64);
		expect(Array.from(m.subarray(0, ctx.length))).toEqual(Array.from(ctx));
		expect(m[ctx.length]).toBe(0xaa);
		expect(m[ctx.length + 32]).toBe(0xbb);
		// Pinned against the host: `punktfunk_core::quic::AUTH_SIG_CONTEXT` is exactly this.
		expect(DEVICE_AUTH_CONTEXT).toBe("punktfunk-device-auth-v1:");
	});

	it("refuses anything that is not two 32-byte values", () => {
		expect(() => signedMessage("aa", "bb".repeat(32))).toThrow();
		expect(() => signedMessage("aa".repeat(32), "")).toThrow();
	});
});

describe("ecdsa", () => {
	it("raw and DER round-trip, including the sign-padding cases", () => {
		for (const raw of [
			new Uint8Array(64).fill(0x11),
			new Uint8Array(64).fill(0xff), // both halves need a pad
			new Uint8Array(64), // zero
			Uint8Array.from({ length: 64 }, (_, i) => i), // r starts 0x00, s does not
		]) {
			const der = rawToDer(raw);
			expect(der[0]).toBe(0x30);
			expect(der[1]).toBe(der.length - 2);
			expect(Array.from(derToRaw(der))).toEqual(Array.from(raw));
		}
	});
});

describe("deviceKey", () => {
	it("exchanges once, then reuses the token", async () => {
		const host = stubHost();
		const signer = recordingSigner();
		const cred = deviceKey({ url: "https://h", hostFingerprint: "cd".repeat(32), signer, fetch: host.fetch });

		const a = await cred.header();
		const b = await cred.header();
		expect(a).toBe(b);
		expect(a.startsWith("Bearer tok-")).toBe(true);
		expect(host.calls()).toEqual(["POST /api/v1/auth/device/challenge", "POST /api/v1/auth/device/token"]);
		// What it signed is the host's message over the binding it was given.
		expect(Array.from(signer.signed[0]!)).toEqual(Array.from(signedMessage("cd".repeat(32), "0".repeat(64))));
	});

	it("runs one exchange for a burst of callers", async () => {
		const host = stubHost();
		const cred = deviceKey({ url: "https://h", hostFingerprint: "cd".repeat(32), signer: recordingSigner(), fetch: host.fetch });
		const headers = await Promise.all([cred.header(), cred.header(), cred.header()]);
		expect(new Set(headers).size).toBe(1);
		expect(host.calls().filter((c) => c.endsWith("/token"))).toHaveLength(1);
	});

	it("surfaces a refusal as DeviceRefused with the host's message", async () => {
		const host = stubHost();
		const signer = recordingSigner();
		// A signer whose nonce the host will not recognise: replay an old one.
		const cred = deviceKey({ url: "https://h", hostFingerprint: "cd".repeat(32), signer, fetch: async (i, init) => {
			if (String(i).endsWith("/challenge")) return Response.json({ nonce: "ff".repeat(32), expires_in: 60 });
			return host.fetch(i, init);
		} });
		await expect(cred.header()).rejects.toBeInstanceOf(DeviceRefused);
		await expect(cred.header()).rejects.toThrow(/pair again/);
	});
});

describe("httpRequest over a credential", () => {
	it("re-earns a device token once when the host has forgotten it", async () => {
		const host = stubHost();
		const conn = connection({
			url: "https://h",
			credential: deviceKey({ url: "https://h", hostFingerprint: "cd".repeat(32), signer: recordingSigner(), fetch: host.fetch }),
			fetch: host.fetch,
		});
		expect(await httpRequest(conn, "GET", "/library")).toEqual({ ok: true, path: "/api/v1/library" });

		host.restart();
		host.reset();
		expect(await httpRequest(conn, "GET", "/status")).toEqual({ ok: true, path: "/api/v1/status" });
		// The 401, a fresh exchange, then the retry — and nothing more.
		expect(host.calls()).toEqual([
			"GET /api/v1/status",
			"POST /api/v1/auth/device/challenge",
			"POST /api/v1/auth/device/token",
			"GET /api/v1/status",
		]);
	});

	it("does not retry a static bearer, which cannot be re-earned", async () => {
		const host = stubHost();
		const conn = connection({ url: "https://h", credential: staticBearer("nope"), fetch: host.fetch });
		host.reset();
		await expect(httpRequest(conn, "GET", "/library")).rejects.toThrow();
		expect(host.calls()).toEqual(["GET /api/v1/library"]);
	});
});
