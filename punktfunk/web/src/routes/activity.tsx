import { createFileRoute } from "@tanstack/react-router";
import { SectionActivity } from "@/sections/Activity";

export const Route = createFileRoute("/activity")({
	component: SectionActivity,
});
