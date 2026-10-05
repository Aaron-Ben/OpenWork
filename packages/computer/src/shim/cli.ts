import { MESSAGE_BODY_MAX, MessageId, ReplyOutcome, RoomId, THREAD_REFUSALS } from "@crew/protocol";
import { Command, CommanderError } from "commander";
import { messageLines } from "../prompt";
import { CliFailure, type CliIo, errorBody, indent, postAgent } from "./io";
import { registerMuteCommands } from "./mutes";
import { registerReminderCommands } from "./reminders";
import { registerTaskCommands } from "./tasks";

export type { CliIo } from "./io";

// `crew` 命令：Agent 在沙箱里用它向 Server 发言。它输出的每一行都会被模型读到，
// 改动措辞后快照 test/__snapshots__/shim-output.md 随之变化，进入代码审查。

const HEREDOC_EXAMPLE = `crew reply <room-id> <<'EOF'
Your message here.
EOF`;

const EXAMPLE_HELP = `\nExample:\n${indent(HEREDOC_EXAMPLE)}\n\nKeep the quotes around 'EOF' so the message is posted exactly as written.\n`;

/** 回复被 HELD 拦下：说明已经写到 stdout，只需要退出码 1。 */
class CliHeld extends Error {}

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
    .argument("<room-id>", "the room or thread to post in, as shown above your unread messages")
    .option("--thread <message-id>", "post in the thread under this message of the room, starting it if needed")
    .addHelpText("after", EXAMPLE_HELP)
    .action(async (roomIdArg: string, options: { thread?: string }) => {
      await reply(roomIdArg, options.thread, io);
    });

  registerTaskCommands(program, io);
  registerReminderCommands(program, io);
  registerMuteCommands(program, io);

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

async function reply(roomIdArg: string, threadArg: string | undefined, io: CliIo): Promise<void> {
  const roomId = RoomId.safeParse(roomIdArg);
  if (!roomId.success) {
    throw new CliFailure(`"${roomIdArg}" is not a room id. Use the id shown above your unread messages.`);
  }
  const threadOf = threadArg === undefined ? undefined : MessageId.safeParse(threadArg);
  if (threadOf && !threadOf.success) {
    throw new CliFailure(`"${threadArg}" is not a message id. Use the id shown in brackets before a message.`);
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

  const response = await postAgent(
    io,
    "/agent/reply",
    { roomId: roomId.data, body, threadOf: threadOf?.data },
    "The message may have been posted; do not send it again.",
  );

  if (response.status !== 200) throw new CliFailure(await failureMessage(response, roomId.data, threadOf?.data));
  const outcome = ReplyOutcome.safeParse(await response.json().catch(() => undefined));
  if (!outcome.success) {
    throw new CliFailure(
      "Crew answered in an unexpected format. The message may have been posted; do not send it again.",
    );
  }
  if (outcome.data.outcome === "posted") {
    io.stdout(
      outcome.data.roomId === roomId.data
        ? `Message sent to room ${roomId.data}.\n`
        : `Message sent to thread ${outcome.data.roomId}, under message ${threadOf?.data ?? "?"}. To post there again, run crew reply ${outcome.data.roomId}.\n`,
    );
    return;
  }
  io.stdout(heldText(roomId.data, outcome.data));
  throw new CliHeld();
}

/** HELD：回复没有发出。把新消息（从最早的开始）与下一步写到 stdout，退出码 1。 */
function heldText(roomId: RoomId, held: Extract<ReplyOutcome, { outcome: "held" }>): string {
  // 带 --thread 时拦下的是讨论串里的消息，`held.roomId` 是讨论串。
  const where = held.roomId === roomId ? `room ${roomId}` : `thread ${held.roomId}`;
  const count = held.newMessages.length + held.omitted;
  const noun = count === 1 ? "message" : "messages";
  const more =
    held.omitted > 0
      ? `\n  (${held.omitted} more new ${held.omitted === 1 ? "message comes" : "messages come"} after these. Running crew reply again shows ${held.omitted === 1 ? "it" : "them"} first.)\n`
      : "";
  return `Not sent: ${count} new ${noun} arrived in ${where} after the ones you were given.

${held.newMessages.map((message) => messageLines(message)).join("\n")}
${more}
Read them and decide again. To post, run crew reply again with a revised or the same message. If nothing needs saying any more, do nothing.
`;
}

/**
 * 把 Server 的拒绝换成 Agent 能据此行动的英文说明。
 * Server 的错误文本是中文，界面也在用，所以在这里翻译，而不是改 Server。
 */
async function failureMessage(response: Response, roomId: RoomId, threadOf: MessageId | undefined): Promise<string> {
  const reason = (await errorBody(response))?.error;
  switch (response.status) {
    case 401:
      return "Crew rejected your token. Crew may have restarted; the message was not posted.";
    case 403:
      return `you are not a member of room ${roomId}. Reply only in the rooms listed in your turn.`;
    case 404:
      return reason === THREAD_REFUSALS.noMessage
        ? `message ${threadOf ?? "?"} is not in room ${roomId}. Start a thread under a message of that room.`
        : `room ${roomId} does not exist. Reply only in the rooms listed in your turn.`;
    default:
      if (reason === THREAD_REFUSALS.direct) {
        return `room ${roomId} is a direct room, and direct rooms have no threads. Reply without --thread.`;
      }
      if (reason === THREAD_REFUSALS.nested) {
        return `${roomId} is a thread, and a thread can't have threads. Reply in it without --thread.`;
      }
      return `Crew refused the message (HTTP ${response.status}${reason ? `: ${reason}` : ""}). The message was not posted.`;
  }
}
