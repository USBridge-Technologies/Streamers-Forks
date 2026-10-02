// How to reach one host: the type every request path takes, with nothing Node in it.
//
// `config.ts` builds one of these from environment and token files, which is a Node job. A
// browser builds one from an origin the user typed and a device key. Both end up here, and
// everything downstream — `http.ts`, `api.ts`, `sse.ts`, the Effect service — takes only this,
// which is what lets `@punktfunk/host/core` exist without `node:*` reachable from it.

import type { Credential } from "./credential.js";

/** Anything shaped like `fetch`. The callable and nothing else: a runtime's own `fetch` has
 *  statics (`preconnect`, on Bun) that a CA-pinning wrapper or a test stub does not, and none of
 *  them are used here. */
export type Fetch = (input: RequestInfo | URL, init?: RequestInit) => Promise<Response>;

export interface Connection {
	/** Management API base URL, no trailing slash. */
	readonly url: string;
	readonly credential: Credential;
	/** PEM of the CA this connection trusts, when the runtime can be told (Node can; a browser
	 *  trusts what the user accepted). Informational here; `fetch` is what actually pins. */
	readonly ca?: string;
	/** A fetch honoring `ca` on this runtime. `globalThis.fetch` where nothing pins. */
	readonly fetch: Fetch;
}

/** A connection from parts, defaulting the fetch. */
export const connection = (
	c: Omit<Connection, "fetch"> & { readonly fetch?: Fetch },
): Connection => ({
	...c,
	url: c.url.replace(/\/+$/, ""),
	fetch: c.fetch ?? globalThis.fetch,
});
