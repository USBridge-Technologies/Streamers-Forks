// **Your monitors** — the host's real heads, one row each (design §5.1, §5.4).
//
// This absorbs the old Streamed screen card: the choice of streaming a real monitor instead of
// a virtual one is a property OF a monitor, so the radio sits on the monitor's row rather than
// in a separate card with its own 214-character introduction.

import { Monitor } from "lucide-react";
import { motion } from "motion/react";
import type { FC, ReactNode } from "react";
import type {
	ApiMonitorInfo,
	DisplayPolicy,
	EffectivePolicy,
} from "@/api/gen/model";
import { ROW, ROW_GAP, staggerProps } from "@/components/stagger";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardTitle } from "@/components/ui/card";
import { cn } from "@/lib/utils";
import { m } from "@/paraglide/messages";

export const MonitorRows: FC<{
	monitors: readonly ApiMonitorInfo[];
	/** The pin the host actually has, env override included. */
	pinned: string | null;
	/** The host can honour a pin at all (`enforced` carries `capture_monitor`). */
	pinSupported: boolean;
	policy?: DisplayPolicy;
	/** In-force policy, so "while streaming" can default from the topology axis. */
	effective?: EffectivePolicy;
	busy?: boolean;
	onPick: (connector: string | null) => void;
	/** Keep this connector lit through an exclusive stream, or stop keeping it (§5.5). */
	onKeepLit?: (connector: string, keep: boolean) => void;
}> = ({
	monitors,
	pinned,
	pinSupported,
	policy,
	effective,
	busy,
	onPick,
	onKeepLit,
}) => {
	// Our own virtual displays show up in the head list on KWin; they are on the map already and
	// are not something to stream FROM.
	const heads = monitors.filter((mon) => !mon.managed);
	// A pin naming a screen the host does not have fails every session until it changes. The rows
	// list only screens the host HAS, so it gets its own row and a warning, or the card would show
	// nothing selected and no hint that anything is wrong.
	const danglingPin =
		pinned &&
		!monitors.some(
			(mon) => mon.connector.toLowerCase() === pinned.toLowerCase(),
		)
			? pinned
			: null;
	if (heads.length === 0 && !danglingPin) return null;
	// `PUNKTFUNK_CAPTURE_MONITOR` outranks the stored policy, so a host pinned in its unit's
	// environment is read-only: offering controls that silently lose to the env is worse than
	// saying nothing.
	const envLocked = !!pinned && !!policy && policy.capture_monitor !== pinned;
	const locked = busy || envLocked;
	// Only `exclusive` turns a monitor off, so only there is "stays on" a real choice —
	// under every other topology every monitor already stays on, and offering the control
	// would be offering one that does nothing.
	const exclusive = effective?.topology === "exclusive";
	const keptLit = new Set(
		(policy?.keep_monitors ?? []).map((c) => c.toLowerCase()),
	);

	return (
		<Card>
			<CardContent className="space-y-3">
				<CardTitle>
					<h2 className="flex items-center gap-2">
						<Monitor className="size-4" />
						{m.display_your_monitors()}
					</h2>
				</CardTitle>
				{envLocked && (
					<p className="text-sm text-amber-600 dark:text-amber-500">
						{m.display_monitor_env_locked()}
					</p>
				)}
				{danglingPin && (
					<p className="text-sm text-destructive">
						{m.display_monitor_missing_warning()}
					</p>
				)}
				{/* `overflow-hidden` because the selected row paints its own square-cornered
				    background: without it that background runs past the rounded corner. */}
				<motion.ul
					{...staggerProps(ROW_GAP)}
					className="divide-y overflow-hidden rounded-md border"
				>
					{pinSupported && (
						<Row
							selected={!pinned}
							disabled={locked}
							title={m.display_stream_virtual()}
							onPick={() => onPick(null)}
						/>
					)}
					{danglingPin && (
						<Row
							selected
							disabled
							title={danglingPin}
							detail={m.display_monitor_missing_hint()}
							badges={
								<Badge variant="destructive">
									{m.display_monitor_missing()}
								</Badge>
							}
						/>
					)}
					{heads.map((mon) => (
						<Row
							key={mon.connector}
							selected={pinned?.toLowerCase() === mon.connector.toLowerCase()}
							// A disabled head cannot be streamed — the host refuses with that reason —
							// but it stays listed so "why isn't my monitor here?" has an answer.
							disabled={locked || !mon.enabled}
							title={`${mon.connector} — ${mon.description}`}
							detail={mon.mode}
							badges={
								<>
									{mon.primary && (
										<Badge variant="secondary">
											{m.display_monitor_primary()}
										</Badge>
									)}
									{!mon.enabled && (
										<Badge variant="outline">
											{m.display_monitor_disabled()}
										</Badge>
									)}
								</>
							}
							// No radio at all where the host cannot honour a pin: the row is then
							// just the inventory it always was.
							onPick={pinSupported ? () => onPick(mon.connector) : undefined}
							trailing={
								exclusive && onKeepLit && mon.enabled ? (
									<StaysOn
										on={keptLit.has(mon.connector.toLowerCase())}
										busy={busy}
										onSet={(keep) => onKeepLit(mon.connector, keep)}
									/>
								) : undefined
							}
						/>
					))}
				</motion.ul>
			</CardContent>
		</Card>
	);
};

/** `Stays on` / `Turns off` for one monitor while a stream owns the screen. */
const StaysOn: FC<{
	on: boolean;
	busy?: boolean;
	onSet: (keep: boolean) => void;
}> = ({ on, busy, onSet }) => (
	<span className="flex shrink-0 gap-1">
		{([true, false] as const).map((keep) => (
			<Button
				key={String(keep)}
				size="sm"
				variant={on === keep ? "default" : "outline"}
				aria-pressed={on === keep}
				disabled={busy}
				onClick={(e) => {
					// The row itself is the streamed-screen picker; this is a different question.
					e.stopPropagation();
					onSet(keep);
				}}
			>
				{keep
					? m.display_q_monitors_extend()
					: m.display_q_monitors_exclusive()}
			</Button>
		))}
	</span>
);

const Row: FC<{
	selected: boolean;
	disabled: boolean;
	title: string;
	detail?: string;
	badges?: ReactNode;
	trailing?: ReactNode;
	onPick?: () => void;
}> = ({ selected, disabled, title, detail, badges, trailing, onPick }) => {
	const body = (
		<>
			<span className="min-w-0 flex-1">
				<span className="flex flex-wrap items-center gap-2 font-medium">
					{title}
					{badges}
				</span>
				{detail && (
					<span className="block text-sm text-muted-foreground">{detail}</span>
				)}
			</span>
			{onPick && (
				<span className="text-sm text-muted-foreground">
					{selected ? m.display_map_streamed() : m.display_stream_this()}
				</span>
			)}
			{trailing}
		</>
	);
	if (!onPick) {
		return (
			<motion.li variants={ROW} className="flex items-center gap-3 px-3 py-2">
				{body}
			</motion.li>
		);
	}
	return (
		<motion.li variants={ROW}>
			<button
				type="button"
				disabled={disabled}
				aria-pressed={selected}
				onClick={onPick}
				className={cn(
					"flex w-full items-center gap-3 px-3 py-2 text-left transition-colors",
					selected ? "bg-primary/10" : "hover:bg-muted/50",
					disabled && "cursor-not-allowed opacity-60",
				)}
			>
				{body}
			</button>
		</motion.li>
	);
};
