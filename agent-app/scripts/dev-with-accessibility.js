#!/usr/bin/env node
/**
 * Run tauri dev but use the .app launcher as runner so the process
 * runs with Accessibility permission (for press_keys).
 * Add AccessPilot-debug.app to System Settings > Accessibility first.
 */
import { spawn } from "child_process";
import { join, dirname } from "path";
import { fileURLToPath } from "url";
import { existsSync, statSync } from "fs";
import { execSync } from "child_process";

const __dirname = dirname(fileURLToPath(import.meta.url));
const agentApp = join(__dirname, "..");
const binary = join(agentApp, "src-tauri/target/debug/accesspilot_agent");
const runner = join(
  agentApp,
  "src-tauri/target/debug/AccessPilot-debug.app/Contents/MacOS/AccessPilot"
);

const appBinary = join(agentApp, "src-tauri/target/debug/AccessPilot-debug.app/Contents/MacOS/accesspilot_agent");
if (existsSync(binary)) {
  const needRefresh = !existsSync(runner) ||
    !existsSync(appBinary) ||
    (existsSync(appBinary) && statSync(binary).mtimeMs > statSync(appBinary).mtimeMs);
  if (needRefresh) {
    try {
      execSync("./scripts/make-debug-app.sh", { cwd: agentApp, stdio: "inherit" });
    } catch (e) {
      // ignore
    }
  }
}

if (!existsSync(runner)) {
  console.error(
    "Run 'npm run tauri dev' once to build, then './scripts/make-debug-app.sh'"
  );
  process.exit(1);
}

const proc = spawn("npx", ["tauri", "dev", "--runner", runner], {
  stdio: "inherit",
  shell: true,
  cwd: agentApp,
});
proc.on("exit", (code) => process.exit(code ?? 0));
