// `@punktfunk/host/core` — the SDK with nothing Node in it.
//
// The generated client and its Schemas, the `PunktfunkHost` service, the typed errors, the
// event-stream decoder, and the credential kinds — everything a program needs to talk to a host
// once it already knows how to reach one. What it lacks is deliberate: resolving a connection
// from environment and token files is `config.ts`, which imports `node:fs`, and the runner,
// plugins and log shipping are Node by nature. Those stay on the package root and `/effect`.
//
// `core-neutral.test.ts` walks the import graph from this file and fails on any `node:`
// specifier, so this entry stays consumable from a browser by construction rather than by
// habit.
//
//   import { api, connection, deviceKey, httpClientFor } from "@punktfunk/host/core";
//
//   const conn = connection({ url, credential: deviceKey({ url, hostFingerprint, signer }) });
//   const program = Effect.gen(function* () {
//     const client = api.make(yield* httpClientFor(conn));
//     return yield* client.getLibrary();
//   });

export {
	ApiError,
	AuthError,
	EventStreamError,
	layerFrom,
	makeService,
	PunktfunkHost,
	type PunktfunkHostService,
	type RequestError,
	SseAuthError,
	TransportError,
	VersionSkew,
} from "./client.js";
export { type Connection, connection, type Fetch } from "./connection.js";
export {
	type Credential,
	DEVICE_AUTH_CONTEXT,
	type DeviceKeyOptions,
	DeviceRefused,
	deviceKey,
	type Signer,
	signedMessage,
	staticBearer,
} from "./credential.js";
export { derToRaw, fromBase64, hexToBytes, rawToDer, toBase64 } from "./ecdsa.js";
export { type HostApi, httpClientFor, makeHostApi } from "./api.js";
export { HttpStatusError, httpRequest } from "./http.js";
export type { EventStreamOptions, SseFrame } from "./sse.js";
export * from "./wire.js";
/**
 * The generated REST surface: wire Schemas plus a typed `HttpClient`-based client (`make`),
 * from `@effect/openapi-generator` over `api/openapi.json`. Regenerate with `bun run gen`; CI
 * fails if this file and the spec disagree.
 */
export * as api from "./gen/punktfunk.js";
