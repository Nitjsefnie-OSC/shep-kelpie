// The command a compact call sends pi, in place of `/compact`: `-p` runs no
// built-in command, but it runs an extension's. The handler returns once the
// compaction has ended, so pi does not exit before it does.
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
}
