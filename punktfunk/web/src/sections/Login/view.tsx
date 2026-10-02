import { ease } from "@unom/style";
import { motion } from "motion/react";
import type { FC } from "react";
import { DocsLink } from "@/components/docs-link";
import Logo from "@/components/logo";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { useLocale } from "@/lib/i18n";
import { m } from "@/paraglide/messages";

/** Why the last attempt did not sign in. The throttle answers before the password is read, so
 * the two are different facts to the person typing, not two flavours of "no". */
export type LoginError =
	| { kind: "wrong" }
	| { kind: "throttled"; seconds: number };

export const LoginView: FC<{
	action: (data: FormData) => void;
	error: LoginError | null;
	busy: boolean;
}> = ({ action, error, busy }) => {
	const locale = useLocale();
	return (
		<div className="flex flex-col min-h-screen items-center justify-center p-6">
			<motion.div
				transition={ease.quint(0.9).out}
				variants={{ enter: { scale: 1, y: 0 }, from: { scale: 0, y: 100 } }}
				className="mb-8 flex w-[120px]"
			>
				<Logo />
			</motion.div>
			<Card className="w-full max-w-sm h-fit grow-0">
				<CardHeader className="items-start text-left">
					<CardTitle className="text-xl">{m.login_title()}</CardTitle>
					<p className="text-sm text-muted-foreground">
						{m.login_subtitle()}{" "}
						<DocsLink path="forgot-password">{m.login_docs_link()}</DocsLink>
					</p>
				</CardHeader>
				<CardContent>
					{/* The button never gates on field content: what is in the field reaches the
					    action as FormData, and `required` alone refuses an empty submit. */}
					<form action={action} className="space-y-4">
						<div className="space-y-2">
							<Label htmlFor="pw">{m.login_password()}</Label>
							<Input
								id="pw"
								name="password"
								type="password"
								autoFocus
								required
								autoComplete="current-password"
							/>
						</div>
						{error && !busy && (
							<p className="text-sm text-destructive" role="alert">
								{error.kind === "throttled"
									? m.login_throttled({
											when: retryWhen(error.seconds, locale),
										})
									: m.login_error()}
							</p>
						)}
						<Button type="submit" className="w-full" disabled={busy}>
							{busy ? m.login_signing_in() : m.login_submit()}
						</Button>
					</form>
				</CardContent>
			</Card>
		</div>
	);
};

/** "in 30 seconds" / "in 5 Minuten" — the platform owns the wording, so a wait costs no strings
 * of ours in either locale. Minutes past a minute: nobody counts down 273 seconds. */
function retryWhen(seconds: number, locale: string): string {
	const fmt = new Intl.RelativeTimeFormat(locale, { numeric: "always" });
	return seconds < 60
		? fmt.format(seconds, "second")
		: fmt.format(Math.ceil(seconds / 60), "minute");
}
