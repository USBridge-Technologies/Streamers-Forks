// The appearance override, read on either side of the render.
//
// Its own module because it reaches for TanStack Start's SERVER entry: a component that
// imports it drags that into Storybook's Vite graph, which has no server entry and fails the
// whole preview build. Only `__root.tsx` needs both sides — the Appearance panel runs in a
// browser and reads the cookie directly.
import { createIsomorphicFn } from "@tanstack/react-start";
import { getRequestHeader } from "@tanstack/react-start/server";
import { appearanceFromHeader, readAppearance } from "./appearance";

/**
 * `createIsomorphicFn` is the framework's own answer to "the server and the browser read this
 * from different places": the server takes it off the request, the browser off `document`. One
 * name, one meaning, and no `typeof window` branch inside a component — which is also what
 * keeps SSR and the first client render agreeing, so there is no flash to correct.
 */
export const currentAppearance = createIsomorphicFn()
	.server(() => appearanceFromHeader(getRequestHeader("cookie")))
	.client(readAppearance);
