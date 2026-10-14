// Kelpie's checks on a fenced pi session: `kelpie confine` on every file
// write and edit, `kelpie guard` on every command, and `kelpie guard --after`
// on every command's result when the project has an `after_command` hook. Each
// reads the call the way Claude Code's PreToolUse hooks hand it over, and exit
// code 2 refuses. Kelpie fills in CHECKS for each call. Anything that fails refuses.
import { spawnSync } from "node:child_process";
import { homedir } from "node:os";
import { resolve } from "node:path";

// WEB names the tools of the web package: empty unless the call loads it.
type Checks = {
	kelpie: string;
	confine: string[];
	guard: string[];
	after: string[] | null;
	web: string[];
};
const CHECKS: Checks = __CHECKS__;

// The path pi's write and edit resolve: it folds odd spaces, strips a
// leading `@` and expands `~`. It also opens a `file:` URL, which is refused.
function resolved(path: string, cwd: string): string | undefined {
	let p = path.replace(/[  -   　]/g, " ");
	if (p.startsWith("@")) p = p.slice(1);
	if (p === "~") p = homedir();
	else if (p.startsWith("~/")) p = homedir() + p.slice(1);
	if (/^file:/i.test(p)) return undefined;
	return resolve(cwd, p);
}

function judged(args: string[], call: object): string | undefined {
	const run = spawnSync(CHECKS.kelpie, args, {
		input: JSON.stringify(call),
		encoding: "utf8",
	});
	if (run.status === 0) return undefined;
	const why = `${run.stderr ?? ""}${run.stdout ?? ""}`.trim();
	return why || `kelpie refused this call${run.error ? `: ${run.error.message}` : ""}`;
}

// The text a write or an edit puts in the file, for the project's `write` hooks.
function written(input: any): string {
	if (typeof input.content === "string") return input.content;
	if (typeof input.newText === "string") return input.newText;
	if (Array.isArray(input.edits)) {
		return input.edits
			.map((e: any) => (typeof e?.newText === "string" ? e.newText : ""))
			.filter((t: string) => t !== "")
			.join("\n");
	}
	return "";
}

// The text of a tool result, as pi hands it over: a list of content blocks.
function printed(content: any): string {
	if (!Array.isArray(content)) return "";
	return content
		.map((block: any) => (block?.type === "text" ? String(block.text ?? "") : ""))
		.join("\n");
}

export default function (pi: any) {
	pi.on("tool_call", async (event: any, ctx: any) => {
		const cwd = ctx?.cwd ?? process.cwd();
		const input = event.input ?? {};
		let why: string | undefined;
		if (event.toolName === "bash") {
			why = judged(CHECKS.guard, {
				cwd,
				tool_name: "Bash",
				tool_input: { command: String(input.command ?? "") },
			});
		} else if (event.toolName === "write" || event.toolName === "edit") {
			const file = resolved(String(input.path ?? ""), cwd);
			why =
				file === undefined
					? "kelpie takes a file's path, not a file: URL"
					: judged(CHECKS.confine, {
							cwd,
							tool_name: event.toolName === "write" ? "Write" : "Edit",
							tool_input: { file_path: file, content: written(input) },
						});
		} else if (!["read", "grep", "find", "ls", ...CHECKS.web].includes(event.toolName)) {
			why = `kelpie runs no ${event.toolName} tool for a worker`;
		}
		if (why !== undefined) return { block: true, reason: why };
	});

	// A command has run by the time its result is here, so a refusal of it is
	// feedback: the reason joins the result, which the model reads as an error.
	pi.on("tool_result", async (event: any, ctx: any) => {
		if (event.toolName !== "bash" || !CHECKS.after) return undefined;
		const cwd = ctx?.cwd ?? process.cwd();
		const text = printed(event.content);
		const exited = /exited with code (\d+)/.exec(text);
		const why = judged(CHECKS.after, {
			cwd,
			tool_name: "Bash",
			tool_input: { command: String(event.input?.command ?? "") },
			tool_response: {
				stdout: text,
				exit_code: exited ? Number(exited[1]) : event.isError ? 1 : 0,
			},
		});
		if (why === undefined) return undefined;
		return {
			content: [...(event.content ?? []), { type: "text", text: `kelpie: ${why}` }],
			isError: true,
		};
	});
}
