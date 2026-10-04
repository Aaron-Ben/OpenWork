// 生成 Seatbelt profile 与 `sandbox-exec` 的参数。规则见设计 Agent Note 的“第 2 步的实现决策 → 沙箱与 shim”。
//
// profile 正文里从不出现路径。每个路径都以 `-D NAME=value` 参数传给 `sandbox-exec`，正文只按名字引用，
// 因此名叫 `x")(allow default` 的目录也只是一个字符串，不会改变规则。

export const SANDBOX_EXEC = "/usr/bin/sandbox-exec";

/** 允许写入的设备。`/dev/fd/<n>` 与 `/dev/ttys<n>` 由正则放行。 */
const WRITABLE_DEVICES = ["/dev/null", "/dev/zero", "/dev/tty"];
const WRITABLE_DEVICE_REGEXES = ["^/dev/fd/[0-9]+$", "^/dev/ttys[0-9]+$"];

export interface Confinement {
  /** 用户主目录的真实路径。其中只有 `homeReadable` 列出的目录能读取内容。 */
  home: string;
  /** 允许写入的目录，必须是真实路径（Seatbelt 按解析后的路径匹配，例如 `/var` 实际是 `/private/var`）。 */
  writable: string[];
  /** `home` 之内允许读取内容的目录。 */
  homeReadable: string[];
}

export interface SeatbeltProfile {
  text: string;
  /** `-D` 参数：名字与值。 */
  parameters: Array<[name: string, value: string]>;
}

/**
 * 生成把进程树约束在 `confinement` 内的 profile：
 * 默认允许，拒绝一切写入后放行 `writable` 与几个设备；`home` 之内只拒绝读取内容（`file-read-data`），
 * 不拒绝 `stat`，因为解析路径与 `realpath` 需要读取上级目录的元数据。网络不限制。
 */
export function buildProfile(confinement: Confinement): SeatbeltProfile {
  const parameters: Array<[string, string]> = [];
  const param = (value: string) => {
    const name = `P${parameters.length}`;
    parameters.push([name, value]);
    return `(param "${name}")`;
  };

  const writeFilters = [
    ...WRITABLE_DEVICES.map((device) => `(literal ${param(device)})`),
    ...WRITABLE_DEVICE_REGEXES.map((regex) => `(regex ${param(regex)})`),
    ...confinement.writable.map((path) => `(subpath ${param(path)})`),
  ];
  const readExceptions = confinement.homeReadable.map((path) => `(require-not (subpath ${param(path)}))`);
  const home = `(subpath ${param(confinement.home)})`;

  const text = [
    "(version 1)",
    "(allow default)",
    "(deny file-write*)",
    `(allow file-write* ${writeFilters.join(" ")})`,
    `(deny file-read-data (require-all ${[home, ...readExceptions].join(" ")}))`,
    "",
  ].join("\n");
  return { text, parameters };
}

/** `sandbox-exec -p <正文> -D ... -- <命令...>` 的参数。 */
export function sandboxArgs(profile: SeatbeltProfile, command: string[]): string[] {
  return [
    "-p",
    profile.text,
    ...profile.parameters.flatMap(([name, value]) => ["-D", `${name}=${value}`]),
    "--",
    ...command,
  ];
}
