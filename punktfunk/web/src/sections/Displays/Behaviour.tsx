// How this host treats a device that connects (design/web-console-overhaul.md D4, §5.3).
//
// The presets are ON THE PAGE. They used to live behind a [ Change ] button, which meant the five
// answers to the page's central question — and the sentence each one produces — were invisible
// until you guessed that a small button held them. A picker you can read without opening anything
// is the whole point of captioning each preset with what it does.
//
// It also puts the preview where it belongs: hovering a preset redraws the map, and the map is now
// beside the cards instead of behind a modal covering it.
//
// Customise stays a dialog. It is five questions and a live sentence — a focused edit, not
// something to read at a glance — and it is the one place the page asks for attention.
//
// What is deliberately absent: the draft buffer, `seeded`, `deepEqual`, the dirty ring, the
// discard prompt, `useBlocker`, the sticky save bar and the tab dot. They existed only to make
// three persistence models coexist on one page (auto-applying preset, Save-gated Custom,
// auto-applying axis). With one model there is nothing left for them to protect: the host
// applies at the next connect either way, so a mid-edit policy cannot disturb a live session.

import { Pencil, Plus, RefreshCw, Trash2 } from "lucide-react";
import { motion } from "motion/react";
import { type FC, type ReactNode, useState } from "react";
import type {
	CustomPreset,
	DisplayPolicy,
	EffectivePolicy,
	Identity,
	KeepAlive,
	ModeConflict,
	Topology,
} from "@/api/gen/model";
import { Stagger } from "@/components/stagger";
import { Button } from "@/components/ui/button";
import { Card, CardContent } from "@/components/ui/card";
import {
	Dialog,
	DialogContent,
	DialogHeader,
	DialogTitle,
} from "@/components/ui/dialog";
import { InputNumber } from "@/components/ui/input-number";
import { cn } from "@/lib/utils";
import { m } from "@/paraglide/messages";
import { describePolicy } from "./describePolicy";

/** Default first (the safe baseline), then the situational ones. */
const PRESET_ORDER = [
	"default",
	"shared-desktop",
	"hotdesk",
	"workstation",
	"gaming-rig",
] as const;

/**
 * A question's own arrival. `Card` and `Button` bring their own `from`/`enter` values; a plain
 * `fieldset` brings none, so a stagger container above it had nothing to animate and the whole
 * editor landed on one frame.
 */
const QUESTION_VARIANTS = {
	from: { opacity: 0, y: 8 },
	enter: { opacity: 1, y: 0 },
};

export interface BehaviourPickerProps {
	/** The stored policy: which preset is ringed, and the base a preset click writes on top of. */
	policy: DisplayPolicy;
	presets: { id: string; summary: string; fields: EffectivePolicy }[];
	customPresets: CustomPreset[];
	/** Apply a whole policy (a preset click). */
	onApply: (policy: DisplayPolicy) => void;
	/** Open the five questions. */
	onCustomise: () => void;
	onSavePreset: () => void;
	onRenamePreset: (p: CustomPreset) => void;
	onUpdatePreset: (p: CustomPreset) => void;
	onDeletePreset: (p: CustomPreset) => void;
	busy?: boolean;
	/** Preview the hovered preset on the map. */
	onPreview?: (fields: EffectivePolicy | undefined) => void;
}

/** The presets, in the open. */
export const BehaviourPicker: FC<BehaviourPickerProps> = ({
	policy,
	presets,
	customPresets,
	onApply,
	onCustomise,
	onSavePreset,
	onRenamePreset,
	onUpdatePreset,
	onDeletePreset,
	busy,
	onPreview,
}) => {
	const current = policy.preset ?? "custom";
	return (
		<div className="space-y-4">
			<Stagger className="grid gap-3 sm:grid-cols-2 lg:grid-cols-3">
				{PRESET_ORDER.map((id) => {
					const p = presets.find((x) => x.id === id);
					if (!p) return null;
					return (
						<PickCard
							key={id}
							selected={current === id}
							busy={busy}
							// Hover and focus both preview: a keyboard user gets the same
							// answer as a mouse user before committing to it.
							onPreview={() => onPreview?.(p.fields)}
							onPreviewEnd={() => onPreview?.(undefined)}
							onPick={() => onApply({ ...policy, preset: id })}
							title={presetLabel(id)}
							caption={describePolicy(p.fields)}
						/>
					);
				})}
				{customPresets.map((p) => (
					<PickCard
						key={p.id}
						selected={false}
						busy={busy}
						onPreview={() => onPreview?.(p.fields)}
						onPreviewEnd={() => onPreview?.(undefined)}
						onPick={() =>
							onApply({
								...policy,
								preset: "custom",
								...p.fields,
								game_session: p.game_session ?? policy.game_session ?? "auto",
							})
						}
						title={p.name}
						caption={describePolicy(p.fields)}
						// Three always-visible icon buttons is where the old tile got busy;
						// they ride the card's footer now, not its header.
						actions={
							<>
								<IconAction
									label={m.display_preset_edit()}
									onClick={() => onRenamePreset(p)}
								>
									<Pencil className="size-3.5" />
								</IconAction>
								<IconAction
									label={m.display_preset_update()}
									onClick={() => onUpdatePreset(p)}
								>
									<RefreshCw className="size-3.5" />
								</IconAction>
								<IconAction
									label={m.display_preset_delete()}
									onClick={() => onDeletePreset(p)}
								>
									<Trash2 className="size-3.5" />
								</IconAction>
							</>
						}
					/>
				))}
				<PickCard
					selected={current === "custom"}
					busy={busy}
					onPick={onCustomise}
					title={m.display_customise()}
					caption={m.display_customise_desc()}
				/>
			</Stagger>

			{/* Customise is a card in the grid above, not a second button down here saying the
			    same word: it is one of the answers, and it reads as one beside the presets. */}
			<div className="flex flex-wrap items-center gap-2">
				<Button
					variant="outline"
					size="sm"
					className="ml-auto"
					disabled={busy}
					onClick={onSavePreset}
				>
					<Plus className="size-4" />
					{m.display_preset_save_as()}
				</Button>
			</div>
		</div>
	);
};

const PickCard: FC<{
	selected: boolean;
	busy?: boolean;
	title: string;
	caption: string;
	actions?: ReactNode;
	onPick: () => void;
	onPreview?: () => void;
	onPreviewEnd?: () => void;
}> = ({
	selected,
	busy,
	title,
	caption,
	actions,
	onPick,
	onPreview,
	onPreviewEnd,
}) => (
	<Card
		interactive
		role="button"
		tabIndex={busy ? -1 : 0}
		aria-pressed={selected}
		aria-disabled={busy || undefined}
		className={cn(selected && "ring-2 ring-primary", busy && "opacity-60")}
		onClick={() => !busy && onPick()}
		onKeyDown={(e) => {
			if (!busy && (e.key === "Enter" || e.key === " ")) {
				e.preventDefault();
				onPick();
			}
		}}
		onMouseEnter={onPreview}
		onMouseLeave={onPreviewEnd}
		onFocus={onPreview}
		onBlur={onPreviewEnd}
	>
		<CardContent className="space-y-1.5">
			<p className="font-medium">{title}</p>
			<p className="text-sm text-muted-foreground">{caption}</p>
			{/* The card is the pick target; its per-preset actions are not — each one stops the
			    event itself rather than a wrapper pretending to be interactive. */}
			{actions && <div className="flex gap-1 pt-1">{actions}</div>}
		</CardContent>
	</Card>
);

const IconAction: FC<{
	label: string;
	onClick: () => void;
	children: ReactNode;
}> = ({ label, onClick, children }) => (
	<Button
		variant="ghost"
		size="icon"
		className="size-7"
		aria-label={label}
		title={label}
		onClick={(e) => {
			e.stopPropagation();
			onClick();
		}}
		onKeyDown={(e) => e.stopPropagation()}
	>
		{children}
	</Button>
);

/** The five questions, in the one place on this page that asks for attention. */
export const CustomiseDialog: FC<{
	open: boolean;
	onOpenChange: (open: boolean) => void;
	effective: EffectivePolicy;
	policy: DisplayPolicy;
	onSetField: (patch: Partial<DisplayPolicy>) => void;
	busy?: boolean;
}> = ({ open, onOpenChange, effective, policy, onSetField, busy }) => (
	<Dialog open={open} onOpenChange={onOpenChange}>
		<DialogContent className="max-h-[85vh] overflow-y-auto sm:max-w-2xl">
			<DialogHeader>
				<DialogTitle>{m.display_customise()}</DialogTitle>
			</DialogHeader>
			<Customise
				effective={effective}
				policy={policy}
				onSetField={onSetField}
				busy={busy}
			/>
			<div className="flex justify-end border-t pt-4">
				<Button variant="outline" onClick={() => onOpenChange(false)}>
					{m.common_done()}
				</Button>
			</div>
		</DialogContent>
	</Dialog>
);

/**
 * The five questions. Every one writes `PUT /display/settings` with a single field on top of
 * the stored policy — the generalised `applyAxis` that three controls already used, now the
 * only write path on the page.
 *
 * The questions inherit their cascade from the dialog, which staggers its sections, so they follow
 * the header instead of running a clock of their own.
 */
const Customise: FC<{
	effective: EffectivePolicy;
	policy: DisplayPolicy;
	onSetField: (patch: Partial<DisplayPolicy>) => void;
	busy?: boolean;
}> = ({ effective, policy, onSetField, busy }) => {
	const keep = effective.keep_alive;
	// Remembered across the Off / Keep toggle so switching back restores the number the operator
	// chose rather than snapping to a default.
	const [seconds, setSeconds] = useState(
		keep.mode === "duration" ? keep.seconds : 10,
	);
	// Switching to Custom pins the currently effective values, or the host would fill unset
	// fields with ITS defaults and the first per-field save would change more than one thing.
	const setField = (patch: Partial<DisplayPolicy>) =>
		onSetField(
			policy.preset === "custom"
				? patch
				: { preset: "custom", ...effective, ...patch },
		);
	return (
		<Stagger className="space-y-5">
			<Question label={m.display_q_keep()}>
				<Segmented
					busy={busy}
					value={keep.mode}
					options={[
						["off", m.display_q_keep_off()],
						["duration", m.display_q_keep_for()],
						["forever", m.display_q_keep_forever()],
					]}
					onPick={(mode) =>
						setField({
							keep_alive:
								mode === "duration"
									? ({ mode, seconds } as KeepAlive)
									: ({ mode } as KeepAlive),
						})
					}
				/>
				{keep.mode === "duration" && (
					<div className="flex items-center gap-2">
						<InputNumber
							aria-label={m.display_keep_alive_seconds()}
							min={0}
							className="w-24"
							value={keep.seconds}
							disabled={busy}
							onChange={(n) => {
								setSeconds(n);
								setField({ keep_alive: { mode: "duration", seconds: n } });
							}}
						/>
						<span className="text-sm text-muted-foreground">
							{m.display_keep_alive_seconds()}
						</span>
					</div>
				)}
			</Question>

			<Question label={m.display_q_monitors()}>
				<Segmented
					busy={busy}
					value={effective.topology}
					options={[
						["extend", m.display_q_monitors_extend()],
						["primary", m.display_q_monitors_primary()],
						["exclusive", m.display_q_monitors_exclusive()],
						["auto", m.display_q_monitors_auto()],
					]}
					onPick={(v) => setField({ topology: v as Topology })}
				/>
			</Question>

			<Question label={m.display_q_second()}>
				<Segmented
					busy={busy}
					value={effective.mode_conflict}
					options={[
						["separate", m.display_q_second_separate()],
						["steal", m.display_q_second_steal()],
						["join", m.display_q_second_join()],
						["reject", m.display_q_second_reject()],
					]}
					onPick={(v) => setField({ mode_conflict: v as ModeConflict })}
				/>
			</Question>

			<Question
				label={m.display_q_remember()}
				help={m.display_q_remember_help()}
			>
				<Segmented
					busy={busy}
					value={effective.identity}
					options={[
						["per-client", m.display_q_remember_client()],
						["per-client-mode", m.display_q_remember_mode()],
						["shared", m.display_q_remember_shared()],
					]}
					onPick={(v) => setField({ identity: v as Identity })}
				/>
			</Question>

			<Question label={m.display_q_max()}>
				{/* 1..=16 is the host's own clamp on write. */}
				<InputNumber
					min={1}
					max={16}
					className="w-24"
					aria-label={m.display_q_max()}
					value={effective.max_displays}
					disabled={busy}
					onChange={(max_displays) => setField({ max_displays })}
				/>
			</Question>

			{/* The sentence updates on every change, which is what replaces "review, then apply":
			    a mid-edit policy has no effect on a live session, and the answer is on screen
			    before the next connect. */}
			<motion.p
				variants={QUESTION_VARIANTS}
				className="rounded-md border bg-muted/40 p-3 text-sm"
			>
				{describePolicy(effective)}
			</motion.p>
		</Stagger>
	);
};

const Question: FC<{ label: string; help?: string; children: ReactNode }> = ({
	label,
	help,
	children,
}) => (
	<motion.fieldset variants={QUESTION_VARIANTS} className="space-y-2">
		<legend className="text-sm font-medium">{label}</legend>
		<div className="flex flex-wrap items-center gap-2">{children}</div>
		{help && <p className="text-xs text-muted-foreground">{help}</p>}
	</motion.fieldset>
);

const Segmented: FC<{
	value: string;
	options: [string, string][];
	onPick: (value: string) => void;
	busy?: boolean;
}> = ({ value, options, onPick, busy }) => (
	<div className="flex flex-wrap gap-2">
		{options.map(([id, label]) => (
			<Button
				key={id}
				size="sm"
				variant={value === id ? "default" : "outline"}
				aria-pressed={value === id}
				disabled={busy}
				onClick={() => onPick(id)}
			>
				{label}
			</Button>
		))}
	</div>
);

const presetLabel = (id: string): string =>
	({
		default: m.display_preset_default(),
		"gaming-rig": m.display_preset_gaming_rig(),
		"shared-desktop": m.display_preset_shared_desktop(),
		hotdesk: m.display_preset_hotdesk(),
		workstation: m.display_preset_workstation(),
	})[id] ?? id;
