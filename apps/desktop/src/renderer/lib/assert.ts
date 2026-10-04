/** 封闭联合的 switch 用它结尾：漏掉一个分支时，类型检查在这里报错。 */
export function assertNever(value: never): never {
  throw new Error(`未处理的分支：${JSON.stringify(value)}`);
}
