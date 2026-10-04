// 报告 ready 后忽略 SIGTERM，只能被 SIGKILL 结束。
process.on("SIGTERM", () => {});
process.stdout.write(`${JSON.stringify({ runtimeSessionId: "s-1" })}\n`);
setInterval(() => {}, 1_000);
