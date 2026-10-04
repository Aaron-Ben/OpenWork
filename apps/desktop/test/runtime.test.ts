import { describe, expect, it } from "vitest";
import { computerEnv } from "../electron/runtime";

describe("computerEnv", () => {
  it("removes database credentials and keeps everything else", () => {
    const env = computerEnv({
      DATABASE_URL: "postgres://crew:crew@localhost:5432/crew",
      REDIS_URL: "redis://localhost:6379/0",
      PGPASSWORD: "secret",
      PATH: "/usr/bin",
      HOME: "/Users/me",
    });
    expect(env).toEqual({ PATH: "/usr/bin", HOME: "/Users/me" });
  });
});
