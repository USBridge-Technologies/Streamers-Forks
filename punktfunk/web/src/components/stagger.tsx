import { type HTMLMotionProps, motion, stagger } from "motion/react";
import type { FC } from "react";

/** The house cadence, in seconds between siblings. */
export const STAGGER_GAP = 0.1;

/** Rows are lighter than cards: a tighter gap, a shorter rise. */
export const ROW_GAP = 0.05;
export const ROW = { from: { opacity: 0, y: 6 }, enter: { opacity: 1, y: 0 } };

/**
 * The stagger-container contract as plain props — for the places that need a specific element
 * (`motion.nav`) rather than the `<Stagger>` div below.
 *
 * The empty `enter`/`from` variants are not a placeholder: a motion element only PROPAGATES a
 * variant it names, so a container that defines none stops the cascade dead and its children never
 * animate at all.
 */
export const staggerProps = (gap: number = STAGGER_GAP) => ({
	variants: { enter: {}, from: {} },
	transition: { delayChildren: stagger(gap) },
});

/**
 * The console's on-mount cadence: siblings arrive one after another, not all on the same frame.
 *
 * WHY THIS EXISTS AS A COMPONENT. Every animated primitive here (`Card`, `Button`, `Checkbox` — all
 * `@unom/ui`) is a motion element whose `from`/`enter` variants are INHERITED from the nearest
 * motion ancestor, and it is that ancestor which owns the timing. `@unom/ui`'s `<Section>` sets
 * `delayChildren: stagger(...)`, which is why a page whose cards are direct descendants of the
 * Section staggers for free.
 *
 * The trap is that an `<AnimatedCard>` is ALSO a motion element, and it sets no `delayChildren` — so
 * a grid of cards nested inside a card becomes its own timing group and every tile lands at once.
 * Nothing in the types catches that; it only shows up in a browser, beside a page that does it
 * right. Wrapping the grid re-establishes the cadence.
 *
 * `root` is for a group with no animating motion ancestor, or one whose ancestor may no longer be
 * driving when it mounts: it runs `from → enter` itself. A tab panel is the clear case; a dialog is
 * not, since `DialogContent` drives its sections. Inside a `<Section>` or a card, leave it off —
 * there it would run on its own clock instead of the page's.
 *
 * A group that mounts late still staggers, through plain wrappers too. A child that mounts after
 * its container animated does not: it enters alone, at once. So a container whose children wait
 * for data renders inside the loaded branch, with them. A child that names a variant label of its
 * own (`exit="exit"` counts) stops inheriting; give it an object instead.
 */
export const Stagger: FC<
	HTMLMotionProps<"div"> & {
		/** Seconds between siblings. */
		gap?: number;
		/** Drive the enter animation instead of inheriting it — see above. */
		root?: boolean;
	}
> = ({ gap, root = false, transition, ...props }) => {
	const base = staggerProps(gap);
	return (
		<motion.div
			{...(root ? { initial: "from", animate: "enter" } : {})}
			variants={base.variants}
			transition={{ ...base.transition, ...transition }}
			{...props}
		/>
	);
};
