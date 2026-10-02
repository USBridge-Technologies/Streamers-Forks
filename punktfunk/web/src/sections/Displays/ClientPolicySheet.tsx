// One device's display settings (design/web-console-overhaul.md §6.2).
//
// The same four questions as the host sheet, each with a **Follow host** position that
// renders the inherited value so the operator can see what following actually means. An
// overlay pins only what they choose; everything else keeps tracking the host, which is why
// this is a field-wise overlay and not a copied policy.
//
// Only the fields the host says it acts on PER DEVICE are rendered (`client_enforced`). An
// axis can be host-wide today and not yet per-device, and a control the host would store and
// ignore is exactly what D1 exists to prevent.
import { useQueryClient } from "@tanstack/react-query";
import { toast } from "@unom/ui/toast";
import type { FC, ReactNode } from "react";
import {
	getGetDisplaySettingsQueryKey,
	useDeleteDisplayClient,
	useGetDisplaySettings,
	useSetDisplayClient,
} from "@/api/gen/display/display";
import type {
	ClientOverlay,
	EffectivePolicy,
	Identity,
	KeepAlive,
	ModeConflict,
	Topology,
} from "@/api/gen/model";
import { Button } from "@/components/ui/button";
import {
	Dialog,
	DialogContent,
	DialogHeader,
	DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { m } from "@/paraglide/messages";
import { describePolicy } from "./describePolicy";

/**
 * The device row's short form: what this device does DIFFERENTLY, in a few words.
 *
 * Not the full sentence — that belongs in the sheet, where there is room for it. A row is
 * scanned, so it carries only the pinned axes, in the same words the sheet uses for them.
 */
export function overlaySummary(overlay: ClientOverlay | undefined): string {
	const pinned = overlay ? stripNulls(overlay) : {};
	const parts: string[] = [];
	if (pinned.topology) parts.push(topologyLabel(pinned.topology));
	if (pinned.mode_conflict) parts.push(conflictLabel(pinned.mode_conflict));
	if (pinned.identity) parts.push(identityLabel(pinned.identity));
	if (pinned.keep_alive) parts.push(keepLabel(pinned.keep_alive));
	if (overlay?.capture_monitor) {
		parts.push(m.display_mirrors({ connector: overlay.capture_monitor }));
	}
	if (overlay?.max_mode)
		parts.push(m.display_capped({ mode: overlay.max_mode }));
	if (overlay?.scale) parts.push(`${overlay.scale}×`);
	return parts.length === 0 ? m.display_follows_host() : parts.join(" · ");
}

const keepLabel = (k: KeepAlive): string => {
	switch (k.mode) {
		case "off":
			return m.display_q_keep_off();
		case "forever":
			return m.display_q_keep_forever();
		default:
			return m.display_state_kept_for({ seconds: k.seconds });
	}
};

/** orval types an absent field as `null | T`; the overlay's own meaning of absent is "follow". */
function stripNulls(overlay: ClientOverlay): Partial<EffectivePolicy> {
	const out: Record<string, unknown> = {};
	for (const [k, v] of Object.entries(overlay)) {
		if (v !== null && v !== undefined) out[k] = v;
	}
	return out as Partial<EffectivePolicy>;
}

export const ClientPolicySheet: FC<{
	open: boolean;
	onOpenChange: (open: boolean) => void;
	/** Pairing fingerprint — what the overlay is keyed by, never an address. */
	fingerprint: string;
	deviceName: string;
}> = ({ open, onOpenChange, fingerprint, deviceName }) => {
	const qc = useQueryClient();
	const settings = useGetDisplaySettings();
	const save = useSetDisplayClient();
	const clear = useDeleteDisplayClient();

	const host = settings.data?.effective;
	const overlay: ClientOverlay = settings.data?.clients?.[fingerprint] ?? {};
	const enforced = settings.data?.client_enforced ?? [];
	const busy = save.isPending || clear.isPending;

	const invalidate = () =>
		qc.invalidateQueries({ queryKey: getGetDisplaySettingsQueryKey() });

	/** The whole overlay is the unit of write: a field dropped here stops being pinned. */
	const write = (patch: ClientOverlay) => {
		const next = { ...stripNulls(overlay), ...patch };
		for (const [k, v] of Object.entries(patch)) {
			if (v === null) delete (next as Record<string, unknown>)[k];
		}
		save.mutate(
			{ fingerprint, data: next },
			{
				onSuccess: () => {
					invalidate();
					toast.success(m.display_settings_saved());
				},
			},
		);
	};

	const followAll = () =>
		clear.mutate(
			{ fingerprint },
			{
				onSuccess: () => {
					invalidate();
					toast.success(m.display_settings_saved());
				},
			},
		);

	const pinned = Object.keys(stripNulls(overlay)).length > 0;

	return (
		<Dialog open={open} onOpenChange={onOpenChange}>
			<DialogContent className="max-h-[85vh] overflow-y-auto sm:max-w-xl">
				<DialogHeader>
					<DialogTitle>{deviceName}</DialogTitle>
				</DialogHeader>

				{host && (
					<div className="space-y-5">
						{enforced.includes("keep_alive") && (
							<Question
								label={m.display_q_keep()}
								inherited={keepLabel(host.keep_alive)}
								pinned={overlay.keep_alive != null}
								busy={busy}
								onFollow={() => write({ keep_alive: null })}
							>
								<Option
									selected={overlay.keep_alive?.mode === "off"}
									busy={busy}
									onPick={() => write({ keep_alive: { mode: "off" } })}
								>
									{m.display_q_keep_off()}
								</Option>
								<Option
									selected={overlay.keep_alive?.mode === "forever"}
									busy={busy}
									onPick={() => write({ keep_alive: { mode: "forever" } })}
								>
									{m.display_q_keep_forever()}
								</Option>
							</Question>
						)}

						{enforced.includes("topology") && (
							<Question
								label={m.display_q_monitors()}
								inherited={topologyLabel(host.topology)}
								pinned={overlay.topology != null}
								busy={busy}
								onFollow={() => write({ topology: null })}
							>
								{(["extend", "primary", "exclusive", "auto"] as const).map(
									(v) => (
										<Option
											key={v}
											selected={overlay.topology === v}
											busy={busy}
											onPick={() => write({ topology: v as Topology })}
										>
											{topologyLabel(v)}
										</Option>
									),
								)}
							</Question>
						)}

						{enforced.includes("mode_conflict") && (
							<Question
								label={m.display_q_second()}
								inherited={conflictLabel(host.mode_conflict)}
								pinned={overlay.mode_conflict != null}
								busy={busy}
								onFollow={() => write({ mode_conflict: null })}
							>
								{(["separate", "steal", "join", "reject"] as const).map((v) => (
									<Option
										key={v}
										selected={overlay.mode_conflict === v}
										busy={busy}
										onPick={() => write({ mode_conflict: v as ModeConflict })}
									>
										{conflictLabel(v)}
									</Option>
								))}
							</Question>
						)}

						{enforced.includes("identity") && (
							<Question
								label={m.display_q_remember()}
								inherited={identityLabel(host.identity)}
								pinned={overlay.identity != null}
								busy={busy}
								onFollow={() => write({ identity: null })}
							>
								{(["per-client", "per-client-mode", "shared"] as const).map(
									(v) => (
										<Option
											key={v}
											selected={overlay.identity === v}
											busy={busy}
											onPick={() => write({ identity: v as Identity })}
										>
											{identityLabel(v)}
										</Option>
									),
								)}
							</Question>
						)}

						{enforced.includes("max_mode") && (
							<Question
								label={m.display_q_max_mode()}
								inherited={m.display_q_max_mode_none()}
								pinned={overlay.max_mode != null}
								busy={busy}
								onFollow={() => write({ max_mode: null })}
							>
								<Input
									aria-label={m.display_q_max_mode()}
									placeholder="2560x1440@60"
									className="w-40 font-mono"
									defaultValue={overlay.max_mode ?? ""}
									disabled={busy}
									// On blur, not per keystroke: half a mode string is not a cap,
									// and the host would refuse to store it anyway.
									onBlur={(e) => {
										const v = e.target.value.trim();
										if (v === (overlay.max_mode ?? "")) return;
										write({ max_mode: v === "" ? null : v });
									}}
								/>
							</Question>
						)}

						{enforced.includes("scale") && (
							<Question
								label={m.display_q_scale()}
								inherited={m.display_q_scale_desktop()}
								pinned={overlay.scale != null}
								busy={busy}
								onFollow={() => write({ scale: null })}
							>
								{[1, 1.25, 1.5, 2].map((v) => (
									<Option
										key={v}
										selected={overlay.scale === v}
										busy={busy}
										onPick={() => write({ scale: v })}
									>
										{`${v}×`}
									</Option>
								))}
							</Question>
						)}

						{/* The sentence this device will actually get, assembled by the same
						    generator as the host's — so the two can never disagree. */}
						<p className="rounded-md border bg-muted/40 p-3 text-sm">
							{describePolicy(
								{ ...host, ...stripNulls(overlay) },
								{ deviceName },
							)}
						</p>

						{pinned && (
							<Button variant="ghost" disabled={busy} onClick={followAll}>
								{m.display_follow_host_all()}
							</Button>
						)}
					</div>
				)}
			</DialogContent>
		</Dialog>
	);
};

const Question: FC<{
	label: string;
	/** What following the host means right now, shown on the Follow position itself. */
	inherited: string;
	pinned: boolean;
	busy?: boolean;
	onFollow: () => void;
	children: ReactNode;
}> = ({ label, inherited, pinned, busy, onFollow, children }) => (
	<fieldset className="space-y-2">
		<legend className="text-sm font-medium">{label}</legend>
		<div className="flex flex-wrap items-center gap-2">
			<Button
				size="sm"
				variant={pinned ? "outline" : "default"}
				aria-pressed={!pinned}
				disabled={busy}
				onClick={onFollow}
			>
				{m.display_follow_host()} · {inherited}
			</Button>
			{children}
		</div>
	</fieldset>
);

const Option: FC<{
	selected: boolean;
	busy?: boolean;
	onPick: () => void;
	children: ReactNode;
}> = ({ selected, busy, onPick, children }) => (
	<Button
		size="sm"
		variant={selected ? "default" : "outline"}
		aria-pressed={selected}
		disabled={busy}
		onClick={onPick}
	>
		{children}
	</Button>
);

const conflictLabel = (v: string): string =>
	({
		separate: m.display_q_second_separate(),
		steal: m.display_q_second_steal(),
		join: m.display_q_second_join(),
		reject: m.display_q_second_reject(),
	})[v] ?? v;

const topologyLabel = (v: string): string =>
	({
		extend: m.display_q_monitors_extend(),
		primary: m.display_q_monitors_primary(),
		exclusive: m.display_q_monitors_exclusive(),
		auto: m.display_q_monitors_auto(),
	})[v] ?? v;

const identityLabel = (v: string): string =>
	({
		"per-client": m.display_q_remember_client(),
		"per-client-mode": m.display_q_remember_mode(),
		shared: m.display_q_remember_shared(),
	})[v] ?? v;
