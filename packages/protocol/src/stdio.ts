/** 能把未知输入校验成 T 的对象。zod schema 满足它，调用方不需要导入 zod 的类型。 */
export interface Parser<T> {
  parse(value: unknown): T;
}

/**
 * 读取第一行并按 schema 校验，用于子进程读取 bootstrap。
 *
 * 参数是逐行的异步迭代器而不是 Node 的 stdin，这样本包不依赖 Node 类型，
 * 测试也可以直接传入字符串数组。读到第一行后结束迭代；对 readline 来说这会关闭它。
 *
 * @throws 输入在第一行之前结束、第一行不是 JSON，或不符合 schema。
 */
export async function readMessage<T>(lines: AsyncIterable<string>, schema: Parser<T>): Promise<T> {
  for await (const line of lines) {
    return schema.parse(JSON.parse(line));
  }
  throw new Error("输入在第一行之前结束");
}

/** 把一条消息编码成一行 JSON。 */
export function encodeMessage(message: unknown): string {
  return `${JSON.stringify(message)}\n`;
}
