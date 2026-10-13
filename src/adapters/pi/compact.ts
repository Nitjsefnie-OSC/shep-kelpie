import { readFileSync } from "node:fs";

// The turn's prompt, to hand back after a compaction pi starts itself, or null.
const PROMPT_FILE: string | null = __PROMPT_FILE__;

// Registers the command a compact call sends pi, in place of `/compact`: `-p`
// runs no built-in command, but it runs an extension's. The handler returns
// once the compaction has ended, so pi does not exit before it does.
//
// A compaction in the middle of a turn summarizes the turn's own prompt. After
// one pi starts (`threshold` or `overflow`), the prompt joins the session
// again as a steered message, ahead of the next request. The `manual` one is
// kelpie's own, which the runner follows with a reminder of its own.
export default function (pi: any) {
	pi.registerCommand("kelpie-compact", {
		description: "Compact the session, and return when it has ended",
		handler: async (_args: string, ctx: any) => {
			await new Promise<void>((resolve, reject) => {
				ctx.compact({
					onComplete: () => resolve(),
					onError: (error: Error) => reject(error),
				});
			});
		},
	});
	if (PROMPT_FILE === null) return;
	pi.on("session_compact", (event: any) => {
		if (event.reason !== "threshold" && event.reason !== "overflow") return;
		pi.sendMessage(
			{ customType: "kelpie-prompt", content: readFileSync(PROMPT_FILE, "utf8"), display: false },
			{ deliverAs: "steer" },
		);
	});
}
