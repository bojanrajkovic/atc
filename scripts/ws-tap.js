// Pipe-friendly WebSocket tap. Connects to a WS URL, prints each incoming
// message on its own line to stdout, exits on close. Uses Node's built-in
// WebSocket (Node 22+; the mise-pinned Node 25 in .mise.toml satisfies that).
//
// Used by the AC11 multi-replica smoke-test runbook in
// docs/architecture/deployment.md — wscat is readline/TTY-bound and silently
// produces no output when stdout is redirected to a file, so any scripted
// single-delivery assertion needs a pipe-friendly client. wscat remains the
// right tool for interactive WebSocket debugging.
//
// Usage:
//   node scripts/ws-tap.js ws://host:port/v1/ws > capture.log
//   mise exec -- node scripts/ws-tap.js ws://host:port/v1/ws > capture.log

const url = process.argv[2];
if (!url) {
	console.error("usage: node scripts/ws-tap.js <ws-url>");
	process.exit(2);
}

const ws = new WebSocket(url);

ws.addEventListener("open", () => {
	process.stderr.write(`[ws-tap] open ${url}\n`);
});

ws.addEventListener("message", (event) => {
	const data = typeof event.data === "string"
		? event.data
		: Buffer.from(event.data).toString("utf-8");
	process.stdout.write(data + "\n");
});

ws.addEventListener("error", (event) => {
	const message = event.message ?? "";
	const error = event.error ? (event.error.message ?? String(event.error)) : "";
	process.stderr.write(`[ws-tap] error type=${event.type} message=${message} error=${error}\n`);
});

ws.addEventListener("close", (event) => {
	process.stderr.write(`[ws-tap] close code=${event.code} reason="${event.reason ?? ""}" wasClean=${event.wasClean}\n`);
	process.exit(0);
});

const shutdown = () => {
	try { ws.close(); } catch (_) { /* already closed */ }
	setTimeout(() => process.exit(0), 50);
};
process.on("SIGINT", shutdown);
process.on("SIGTERM", shutdown);
