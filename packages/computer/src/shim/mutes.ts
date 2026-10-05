import { assertNever, MUTE_MAX_MINUTES, MUTE_MIN_MINUTES, MuteRefusal, MuteState, type RoomId } from "@crew/protocol";
import type { Command } from "commander";
import { localTimestamp } from "../prompt";
import { CliFailure, type CliIo, errorBody, postAgent } from "./io";
import { parseDuration, parseRoom } from "./reminders";

// `crew mute` 与 `crew unmute`：Agent 让一个群的消息不再唤醒它。输出的每一行都会被模型读到，
// 由快照 test/__snapshots__/shim-output.md 逐字锁定。

const UNSURE = "The mute may have been changed; send the same command again to be sure.";
const STILL_WAKES =
  "you still wake when someone mentions you, in threads you follow, for tasks assigned to you and for your own reminders";

export function registerMuteCommands(program: Command, io: CliIo): void {
  program
    .command("mute")
    .description("Stop a group's messages from waking you. Mentions, your threads, your tasks and reminders still do.")
    .argument("<room-id>", "the group room to mute")
    .option("--for <duration>", "unmute by itself after a while: 30m, 2h, 1d (15m to 7d); without it, until you unmute")
    .action(async (roomArg: string, options: { for?: string }) => {
      const roomId = parseRoom(roomArg);
      let minutes: number | undefined;
      if (options.for !== undefined) {
        minutes = parseDuration(options.for, "--for");
        if (minutes < MUTE_MIN_MINUTES || minutes > MUTE_MAX_MINUTES) {
          throw new CliFailure(`--for must be between ${MUTE_MIN_MINUTES}m and 7d.`);
        }
      }
      const state = MuteState.parse(await call(io, "/agent/rooms/mute", { roomId, minutes }, roomId));
      const until = state.until
        ? `until ${localTimestamp(new Date(state.until))}`
        : `until you run crew unmute ${roomId}`;
      io.stdout(
        `Muted ${roomId} ${until}. Its messages no longer wake you; ${STILL_WAKES}. When you wake there, you get what you missed.\n`,
      );
    });

  program
    .command("unmute")
    .description("Let a group's messages wake you again.")
    .argument("<room-id>", "the group room to unmute")
    .action(async (roomArg: string) => {
      const roomId = parseRoom(roomArg);
      MuteState.parse(await call(io, "/agent/rooms/unmute", { roomId }, roomId));
      io.stdout(`Unmuted ${roomId}. Its messages wake you again.\n`);
    });
}

async function call(io: CliIo, path: string, body: unknown, roomId: RoomId): Promise<unknown> {
  const response = await postAgent(io, path, body, UNSURE);
  if (response.status === 200) return response.json();
  const error = await errorBody(response);
  const refusal = MuteRefusal.safeParse(error?.refusal);
  if (refusal.success) throw new CliFailure(refusalText(refusal.data));
  switch (response.status) {
    case 401:
      throw new CliFailure("Crew rejected your token. Crew may have restarted; nothing was changed.");
    case 403:
      throw new CliFailure(`you are not a member of ${roomId}. Use the rooms listed in your turn.`);
    case 404:
      throw new CliFailure(`room ${roomId} does not exist. Use the rooms listed in your turn.`);
    default:
      throw new CliFailure(
        `Crew refused the change (HTTP ${response.status}${error ? `: ${error.error}` : ""}). Nothing was changed.`,
      );
  }
}

function refusalText(refusal: MuteRefusal): string {
  switch (refusal.code) {
    case "mute_direct":
      return "a direct room can't be muted.";
    case "mute_thread":
      return "a thread can't be muted on its own. Mute the group it is in.";
    default:
      return assertNever(refusal);
  }
}
