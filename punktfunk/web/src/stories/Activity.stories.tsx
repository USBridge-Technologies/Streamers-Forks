import type { Meta, StoryObj } from "@storybook/react-vite";
import {
	createMemoryHistory,
	createRootRoute,
	createRoute,
	createRouter,
	RouterProvider,
} from "@tanstack/react-router";
import { useEffect, useMemo, useState } from "react";
import type { ActivityEntry } from "@/api/events";
import {
	ActivityCardView,
	ActivityList,
	ActivityPage,
} from "@/sections/Activity";

/**
 * The activity feed (design/web-console-overhaul.md §4).
 *
 * Two things here only show up with a busy host, which is why they get a story rather than a
 * test: the dashboard card **caps** at a dozen rows — it used to render the whole 200-entry ring
 * and push the rest of the dashboard off the screen — and the rows **arrive on a cadence**
 * instead of all on one frame. A screenshot pins the first; the second is why the list is a
 * motion container at all (`components/stagger.tsx`), and a reviewer sees it by opening this.
 */
const at = (mins: number) => Date.UTC(2026, 8, 10, 14, 0) - mins * 60_000;

const KINDS: [string, Record<string, unknown>][] = [
	["client.connected", { client: { name: "Living room TV" } }],
	["session.started", { session: { client: { name: "Living room TV" } } }],
	["stream.started", { stream: { app: "Hades II" } }],
	["game.running", { game: "Hades II" }],
	["display.created", { client: { name: "Living room TV" } }],
	["pairing.pending", { client: { name: "enrico-phone" } }],
	["pairing.completed", { client: { name: "enrico-phone" } }],
	["game.exited", { game: "Hades II" }],
	["stream.stopped", { reason: "client left" }],
	["session.ended", { session: { client: { name: "Living room TV" } } }],
	["display.released", { client: { name: "Living room TV" } }],
	["client.disconnected", { client: { name: "Living room TV" } }],
	["library.changed", {}],
	["plugins.changed", {}],
	["pairing.denied", { client: { name: "unknown-host" } }],
	["update.available", {}],
	["host.started", {}],
];

/** Newest first, the way the ring hands them over. */
const entries: ActivityEntry[] = KINDS.map(([kind, data], i) => ({
	seq: KINDS.length - i,
	ts_ms: at(i * 3),
	kind,
	data,
}));

// The card's "Show all" is a router <Link>, so the story needs a router to resolve it against.
// Built once: a router rebuilt on every render remounts everything under it, which restarts the
// entrance animation this story exists to show.
function Routed({ children }: { children: React.ReactNode }) {
	const router = useMemo(() => {
		const rootRoute = createRootRoute({ component: () => <>{children}</> });
		const activityRoute = createRoute({
			getParentRoute: () => rootRoute,
			path: "/activity",
			component: () => null,
		});
		return createRouter({
			routeTree: rootRoute.addChildren([activityRoute]),
			history: createMemoryHistory({ initialEntries: ["/"] }),
		});
	}, [children]);
	// biome-ignore lint/suspicious/noExplicitAny: a throwaway router, not the app's typed tree
	return <RouterProvider router={router as any} />;
}

const meta = {
	title: "Console/Activity",
	parameters: { layout: "padded" },
} satisfies Meta;
export default meta;

type Story = StoryObj<typeof meta>;

/** The dashboard card: twelve rows out of seventeen, and the way to the rest. */
export const Card: Story = {
	render: () => (
		<Routed>
			<div className="max-w-3xl">
				<ActivityCardView entries={entries} />
			</div>
		</Routed>
	),
};

/** `/activity`: the whole ring. */
export const Page: Story = {
	render: () => (
		<Routed>
			<ActivityPage entries={entries} />
		</Routed>
	),
};

/**
 * The rows on their own, with no router and no card around them.
 *
 * This is the one that answers "does the cadence actually run": the stories above mount behind a
 * memory router, which lands them after the page's stagger clock has already run, and a group
 * that inherits its timing from an ancestor arrives flat. That is a property of the harness, not
 * of the list — but only a story without the harness can say so.
 */
export const Rows: Story = {
	render: () => (
		<div className="max-w-3xl">
			<ActivityList entries={entries} />
		</div>
	),
};

/**
 * A host that is busy RIGHT NOW: the card starts at its cap and an event lands every 900 ms, so
 * every arrival also evicts the oldest row. This is the only state where the card's height and
 * the rows' movement can go wrong — both only happen while something is arriving, which is why
 * the static stories above never showed it.
 */
export const Live: Story = {
	render: function LiveFeed() {
		const [feed, setFeed] = useState(entries);
		useEffect(() => {
			let seq = entries.length;
			const t = setInterval(() => {
				seq += 1;
				const next = KINDS[seq % KINDS.length];
				if (!next) return;
				const [kind, data] = next;
				setFeed((f) => [{ seq, ts_ms: Date.now(), kind, data }, ...f]);
			}, 900);
			return () => clearInterval(t);
		}, []);
		return (
			<Routed>
				<div className="max-w-3xl">
					<ActivityCardView entries={feed} />
				</div>
			</Routed>
		);
	},
};

/**
 * What a client CONNECTING looks like: five events inside the same instant — connected, session,
 * stream, display, game — every two seconds. Each one evicts a row, so a burst evicts five at
 * once, and five rows mid-exit is the one situation that can make the card taller than its cap.
 * The one-per-900-ms story above never produces it.
 */
export const Burst: Story = {
	render: function BurstFeed() {
		const [feed, setFeed] = useState(entries);
		useEffect(() => {
			let seq = entries.length;
			const t = setInterval(() => {
				const now = Date.now();
				const batch: ActivityEntry[] = KINDS.slice(0, 5).map(
					([kind, data], i) => {
						seq += 1;
						return { seq, ts_ms: now + i, kind, data };
					},
				);
				setFeed((f) => [...batch.reverse(), ...f]);
			}, 2000);
			return () => clearInterval(t);
		}, []);
		return (
			<Routed>
				<div className="max-w-3xl">
					<ActivityCardView entries={feed} />
				</div>
			</Routed>
		);
	},
};

/** How many ring events the Reload story replays — a busy host's ring holds up to 1024. */
const REPLAY = 300;

/**
 * A page LOAD. The stream replays the host's whole ring the moment it connects, each frame
 * dispatched as its own task — so each one is its own render, and every event past the twelfth
 * evicts a row into an exit animation. None of the stories above exercise that path, and it is
 * the one that stretched the real card below the fold and froze the page on reload.
 */
export const Reload: Story = {
	render: function ReloadFeed() {
		const [feed, setFeed] = useState<ActivityEntry[]>([]);
		useEffect(() => {
			let i = 0;
			let timer: ReturnType<typeof setTimeout> | undefined;
			// One frame per TASK, the way EventSource dispatches them. The same pushes in one
			// loop would be batched into a single render and hide the whole problem.
			const next = () => {
				const pick = KINDS[i % KINDS.length];
				if (!pick) return;
				const [kind, data] = pick;
				const seq = i + 1;
				const ts_ms = at(REPLAY - i);
				setFeed((f) => [{ seq, ts_ms, kind, data }, ...f].slice(0, 200));
				i += 1;
				if (i < REPLAY) timer = setTimeout(next, 0);
			};
			timer = setTimeout(next, 0);
			return () => clearTimeout(timer);
		}, []);
		return (
			<Routed>
				<div className="max-w-3xl">
					<ActivityCardView entries={feed} />
				</div>
			</Routed>
		);
	},
};

/** Nothing has happened yet — a fresh page load on a quiet host. */
export const Empty: Story = {
	render: () => (
		<Routed>
			<div className="max-w-3xl">
				<ActivityCardView entries={[]} />
			</div>
		</Routed>
	),
};
