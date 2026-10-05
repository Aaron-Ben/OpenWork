import { RUN_TEXT_MAX } from "@crew/protocol";
import { describe, expect, it } from "vitest";
import { clip, engineEventOf } from "../src/engine/opencode";

// OpenCode 输出的事件换成运行记录的 Engine 事件。样本取自 2026-10-05 用 OpenCode 1.18.18 的一次真实运行，
// 路径换成了 /work。

const toolUse =
  '{"type":"tool_use","timestamp":1791184019332,"sessionID":"ses_1","part":{"type":"tool","tool":"bash","callID":"call_1","state":{"status":"completed","input":{"command":"ls"},"output":"a.txt\\nerr.txt\\n","metadata":{"exit":0},"title":"ls","time":{"start":1791184019310,"end":1791184019325}},"id":"prt_1","sessionID":"ses_1","messageID":"msg_1"}}';
const stepFinish =
  '{"type":"step_finish","timestamp":1791184019332,"sessionID":"ses_1","part":{"id":"prt_2","reason":"tool-calls","messageID":"msg_1","sessionID":"ses_1","type":"step-finish","tokens":{"total":9213,"input":7487,"output":37,"reasoning":25,"cache":{"write":0,"read":1664}},"cost":0.001165242}}';
const text =
  '{"type":"text","timestamp":1791184021965,"sessionID":"ses_1","part":{"id":"prt_3","messageID":"msg_2","sessionID":"ses_1","type":"text","text":"hello","time":{"start":1791184021940,"end":1791184021947}}}';
const at = (ms: number) => new Date(ms).toISOString();

describe("engineEventOf", () => {
  it("turns a finished tool call into a tool event with its input, output and duration", () => {
    expect(engineEventOf(toolUse)).toEqual({
      kind: "tool",
      at: at(1791184019332),
      tool: "bash",
      title: "ls",
      input: '{"command":"ls"}',
      output: "a.txt\nerr.txt\n",
      durationMs: 15,
      failed: false,
    });
  });

  it("takes the usage and cost from the end of a step", () => {
    expect(engineEventOf(stepFinish)).toEqual({
      kind: "step_end",
      at: at(1791184019332),
      usage: { input: 7487, output: 37, reasoning: 25, cacheRead: 1664, cacheWrite: 0, cost: 0.001165242 },
    });
  });

  it("keeps the model's text and marks the start of a step", () => {
    expect(engineEventOf(text)).toEqual({ kind: "text", at: at(1791184021965), text: "hello" });
    expect(
      engineEventOf('{"type":"step_start","timestamp":1,"sessionID":"ses_1","part":{"type":"step-start"}}'),
    ).toEqual({
      kind: "step",
      at: at(1),
    });
  });

  it("ignores lines that are not events or not part of the run record", () => {
    expect(engineEventOf("not json")).toBeUndefined();
    expect(engineEventOf('{"type":"error","sessionID":"ses_1"}')).toBeUndefined();
  });

  it("marks a failed tool call and tolerates missing timing", () => {
    const failed =
      '{"type":"tool_use","timestamp":5,"part":{"tool":"read","state":{"status":"error","input":{},"output":"no such file"}}}';
    expect(engineEventOf(failed)).toMatchObject({ kind: "tool", tool: "read", failed: true, durationMs: null });
  });
});

describe("clip", () => {
  it("does not split an emoji at the cut", () => {
    const text = `${"x".repeat(RUN_TEXT_MAX - 41)}😀${"y".repeat(RUN_TEXT_MAX)}`;
    expect(clip(text)).toMatch(/^x+\n/);
  });

  it("leaves short text alone and cuts long text to the limit with a note", () => {
    expect(clip("短")).toBe("短");
    const clipped = clip("x".repeat(RUN_TEXT_MAX * 2));
    expect(clipped.length).toBeLessThanOrEqual(RUN_TEXT_MAX);
    expect(clipped).toMatch(new RegExp(`截掉 ${RUN_TEXT_MAX * 2 - (RUN_TEXT_MAX - 40)} 字符）$`));
  });
});
