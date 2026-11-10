import { defineConfig } from "@playwright/test";
import { tmpdir } from "node:os";
import { join } from "node:path";

// The back office against a location end to end on the host: the cell, its hub and a register,
// in `keel-cloud`'s demo, which the tests start. `KEEL_DEMO` is its binary, built beforehand;
// `KEEL_DEMO_DIR` where it keeps its stores, its token and the register's profile version.
const port = Number(process.env.KEEL_PORT ?? 8787);
export const demoDir = process.env.KEEL_DEMO_DIR ?? join(tmpdir(), "keel-backoffice-demo");
const demo = process.env.KEEL_DEMO ?? "../../../target/debug/examples/demo";

export default defineConfig({
  testDir: "e2e",
  fullyParallel: false,
  workers: 1,
  timeout: 90_000,
  expect: { timeout: 15_000 },
  reporter: [["list"]],
  use: { baseURL: `http://127.0.0.1:${port}`, trace: "off" },
  webServer: {
    command: `${demo} ${demoDir} 127.0.0.1:${port} dist`,
    url: `http://127.0.0.1:${port}/`,
    reuseExistingServer: false,
    timeout: 60_000,
    stdout: "pipe",
  },
});
