import { MESSAGE_BODY_MAX, ReplyOutcome, RoomId } from "@crew/protocol";
import { Command, CommanderError } from "commander";
import { z } from "zod";
import { messageLines } from "../prompt";

// `crew` 命令：Agent 在沙箱里用它向 Server 发言。它输出的每一行都会被模型读到，
// 改动措辞后快照 test/__snapshots__/shim-output.md 随之变化，进入代码审查。

/** shim 与外界打交道的全部途径。入口传入真实的 process，测试传入假的。 */
export interface CliIo {
  env: Record<string, string | undefined>;
  readStdin(): Promise<string>;
  readFile(path: string): Promise<string>;
  stdout(text: string): void;
  stderr(text: string): void;
  fetch: typeof fetch;
}

/** 等待 Server 响应的上限。超时后不重试：第 2 步没有幂等，重试可能发出重复消息。 */
const REQUEST_TIMEOUT_MS = 10_000;

const HEREDOC_EXAMPLE = `crew reply <room-id> <<'EOF'
Your message here.
EOF`;

const EXAMPLE_HELP = `\nExample:\n${indent(HEREDOC_EXAMPLE)}\n\nKeep the quotes around 'EOF' so the message is posted exactly as written.\n`;

/** 一次可以向 Agent 解释的失败：写一行 `error: …` 到 stderr，退出码 1。 */
class CliFailure extends Error {}

/** 回复被 HELD 拦下：说明已经写到 stdout，只需要退出码 1。 */
class CliHeld extends Error {}

const ErrorBody = z.object({ error: z.string() });

/** 运行 `crew`，返回退出码。`args` 不含可执行文件与脚本路径。 */
export async function runCli(args: string[], io: CliIo): Promise<number> {
  const program = new Command("crew")
    .description("Talk in your Crew rooms.")
    .exitOverride()
    .configureOutput({ writeOut: io.stdout, writeErr: io.stderr })
    .showHelpAfterError()
    .helpCommand(false)
    .addHelpText("after", EXAMPLE_HELP);

  program
    .command("reply")
    .description("Post a message to a room. The message is read from standard input.")
    .argument("<room-id>", "the room to post in, as shown above your unread messages")
    .addHelpText("after", EXAMPLE_HELP)
    .action(async (roomIdArg: string) => {
      await reply(roomIdArg, io);
    });

  try {
    // 参数来自 `process.argv.slice(2)`，按 "user" 解析。不让 commander 自己判断：
    // 以 ELECTRON_RUN_AS_NODE 运行时它会把 argv 当成 Electron 应用的格式，多出脚本路径。
    await program.parseAsync(args, { from: "user" });
    return 0;
  } catch (error) {
    // commander 已经把它的错误或帮助写到了 stderr。
    if (error instanceof CommanderError) return error.exitCode;
    if (error instanceof CliHeld) return 1;
    const message = error instanceof CliFailure ? error.message : `unexpected failure: ${String(error)}`;
    io.stderr(`error: ${message}\n`);
    return 1;
  }
}

async function reply(roomIdArg: string, io: CliIo): Promise<void> {
  const roomId = RoomId.safeParse(roomIdArg);
  if (!roomId.success) {
    throw new CliFailure(`"${roomIdArg}" is not a room id. Use the id shown above your unread messages.`);
  }

  // heredoc 末尾总有一个换行，去掉末尾的空白；开头的缩进保留。
  const body = (await io.readStdin()).trimEnd();
  if (body.trim().length === 0) {
    throw new CliFailure(`no message on standard input. Pass it with a heredoc:\n${indent(HEREDOC_EXAMPLE)}`);
  }
  if (body.length > MESSAGE_BODY_MAX) {
    throw new CliFailure(
      `the message has ${body.length} characters; the limit is ${MESSAGE_BODY_MAX}. Shorten it or split it into several replies.`,
    );
  }

  const { serverUrl, token } = await credentials(io);
  let response: Response;
  try {
    response = await io.fetch(new URL("/agent/reply", serverUrl), {
      method: "POST",
      headers: { Authorization: `Bearer ${token}`, "Content-Type": "application/json" },
      body: JSON.stringify({ roomId: roomId.data, body }),
      signal: AbortSignal.timeout(REQUEST_TIMEOUT_MS),
    });
  } catch (error) {
    if (error instanceof DOMException && error.name === "TimeoutError") {
      throw new CliFailure(
        `Crew did not answer within ${REQUEST_TIMEOUT_MS / 1000} seconds. The message may have been posted; do not send it again.`,
      );
    }
    throw new CliFailure(`could not reach Crew (${String(error)}). The message was not posted.`);
  }

  if (response.status !== 200) throw new CliFailure(await failureMessage(response, roomId.data));
  const outcome = ReplyOutcome.safeParse(await response.json().catch(() => undefined));
  if (!outcome.success) {
    throw new CliFailure(
      "Crew answered in an unexpected format. The message may have been posted; do not send it again.",
    );
  }
  if (outcome.data.outcome === "posted") {
    io.stdout(`Message sent to room ${roomId.data}.\n`);
    return;
  }
  io.stdout(heldText(roomId.data, outcome.data));
  throw new CliHeld();
}

/** HELD：回复没有发出。把新消息（从最早的开始）与下一步写到 stdout，退出码 1。 */
function heldText(roomId: RoomId, held: Extract<ReplyOutcome, { outcome: "held" }>): string {
  const count = held.newMessages.length + held.omitted;
  const noun = count === 1 ? "message" : "messages";
  const more =
    held.omitted > 0
      ? `\n  (${held.omitted} more new ${held.omitted === 1 ? "message comes" : "messages come"} after these. Running crew reply again shows ${held.omitted === 1 ? "it" : "them"} first.)\n`
      : "";
  return `Not sent: ${count} new ${noun} arrived in room ${roomId} after the ones you were given.

${held.newMessages.map((message) => messageLines(message)).join("\n")}
${more}
Read them and decide again. To post, run crew reply again with a revised or the same message. If nothing needs saying any more, do nothing.
`;
}

/** Server 的地址与本 Agent 的凭证，由 Computer 经环境变量与运行期文件提供。 */
async function credentials(io: CliIo): Promise<{ serverUrl: string; token: string }> {
  const outside = new CliFailure("crew must be run inside a Crew agent turn.");
  const serverUrl = io.env.CREW_SERVER_URL;
  const tokenFile = io.env.CREW_TOKEN_FILE;
  if (!serverUrl || !tokenFile) throw outside;
  let token: string;
  try {
    token = (await io.readFile(tokenFile)).trim();
  } catch {
    // 凭证文件不存在或读不到，原因对 Agent 没有用：它只能知道自己不在一轮 Turn 里。
    throw outside;
  }
  if (!token) throw outside;
  return { serverUrl, token };
}

/**
 * 把 Server 的拒绝换成 Agent 能据此行动的英文说明。
 * Server 的错误文本是中文，界面也在用，所以在这里翻译，而不是改 Server。
 */
async function failureMessage(response: Response, roomId: RoomId): Promise<string> {
  switch (response.status) {
    case 401:
      return "Crew rejected your token. Crew may have restarted; the message was not posted.";
    case 403:
      return `you are not a member of room ${roomId}. Reply only in the rooms listed in your turn.`;
    case 404:
      return `room ${roomId} does not exist. Reply only in the rooms listed in your turn.`;
    default: {
      const parsed = ErrorBody.safeParse(await response.json().catch(() => undefined));
      const detail = parsed.success ? `: ${parsed.data.error}` : "";
      return `Crew refused the message (HTTP ${response.status}${detail}). The message was not posted.`;
    }
  }
}

function indent(text: string): string {
  return text
    .split("\n")
    .map((line) => `  ${line}`)
    .join("\n");
}
