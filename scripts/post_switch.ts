#!/usr/bin/env bun

import { $ } from "bun";

// Ensure all console output is written directly to stdout/stderr
function stringifyConsoleArg(arg: unknown): string {
  if (typeof arg === "string") return arg;
  try {
    return JSON.stringify(arg, null, 2);
  } catch {
    return String(arg);
  }
}

function writeConsoleLine(stream: NodeJS.WriteStream, parts: unknown[]): void {
  const line = parts.map(stringifyConsoleArg).join(" ");
  try {
    stream.write(line + "\n");
  } catch {
    // ignore
  }
}

console.log = (...args: unknown[]) => {
  writeConsoleLine(process.stdout, args);
};
console.warn = (...args: unknown[]) => {
  writeConsoleLine(process.stderr, args);
};
console.error = (...args: unknown[]) => {
  writeConsoleLine(process.stderr, args);
};

process.on("unhandledRejection", (reason) => {
  console.error("[post-switch][UNHANDLED_REJECTION]", reason);
  process.exitCode = 1;
});
process.on("uncaughtException", (error) => {
  console.error("[post-switch][UNCAUGHT_EXCEPTION]", error);
  process.exitCode = 1;
});

function log(msg: string) {
  console.log(`[post-switch] ${msg}`);
}
function warn(msg: string) {
  console.error(`[post-switch][WARN] ${msg}`);
}
function err(msg: string) {
  console.error(`[post-switch][ERROR] ${msg}`);
}

async function sleep(ms: number) {
  return new Promise((res) => setTimeout(res, ms));
}

// Generic async retry helper for commands that return an object with exitCode/text
async function retryAsync(
  attempts: number,
  fn: () => Promise<{ exitCode: number; text(): string }>,
  description?: string
): Promise<boolean> {
  let delayMs = 250;
  for (let i = 1; i <= attempts; i++) {
    const res = await fn();
    if (res.exitCode === 0) return true;
    if (i === attempts) break;
    if (description) {
      warn(
        `${description} attempt ${i} failed (exit ${res.exitCode}). Retrying in ${delayMs}ms`
      );
    }
    await sleep(delayMs);
    delayMs = Math.min(delayMs * 2, 2000);
  }
  return false;
}

async function waitForProcessToDisappear(
  processName: string,
  timeoutMs: number
): Promise<boolean> {
  const start = Date.now();
  for (;;) {
    const r = await $`pgrep -x ${processName}`.nothrow();
    if (r.exitCode !== 0) return true;
    if (Date.now() - start >= timeoutMs) return false;
    await sleep(100);
  }
}

async function retry<T>(
  attempts: number,
  fn: () => { status: number; stdout: string; stderr: string }
): Promise<{ status: number; stdout: string; stderr: string }> {
  let delay = 1000;
  for (let i = 1; i <= attempts; i++) {
    const res = fn();
    if (res.status === 0) return res;
    if (i === attempts) return res;
    warn(
      `Attempt ${i} failed (exit ${res.status}). Retrying in ${delay / 1000}s`
    );
    await sleep(delay);
    delay *= 2;
  }
  return { status: 1, stdout: "", stderr: "retry exhausted" };
}

// Raycast CLI setup removed (not needed for this feature)

function getUserContext() {
  const userName = process.env.SUDO_USER || process.env.USER || "";
  const userHome = userName ? `/Users/${userName}` : process.env.HOME || "";
  const uid =
    typeof (process as any).getuid === "function"
      ? (process as any).getuid()
      : undefined;
  const env: NodeJS.ProcessEnv = {
    ...process.env,
    HOME: userHome,
    USER: userName,
    LOGNAME: userName,
    PATH: `/opt/homebrew/bin:/usr/local/bin:${process.env.PATH || ""}`,
    OP_BIOMETRIC_UNLOCK_ENABLED: "true",
  };
  return { userName, userHome, uid, env };
}

async function configureGitSigningFrom1Password() {
  const { userName, env: userEnv } = getUserContext();

  // Ensure PATH includes common Homebrew locations
  userEnv.PATH = `/opt/homebrew/bin:/usr/local/bin:${process.env.PATH || ""}`;

  log(
    `Reading Git signing public key from 1Password for user ${
      userName || "(unknown)"
    }`
  );

  let pubkey = "";
  try {
    pubkey = (
      await $`sudo -u ${userName} -E op read "op://Personal/GitHub/public key"`.text()
    ).trim();
    console.log(pubkey);
  } catch (e) {
    console.log(e);
    warn(
      "Could not read key from 1Password. Ensure the 1Password CLI is unlocked."
    );
    return;
  }

  pubkey = pubkey
    .replace(/\r/g, " ")
    .replace(/\n/g, " ")
    .replace(/\s+/g, " ")
    .trim();

  if (!pubkey) {
    warn("Empty key read from 1Password");
    return;
  }
  if (!/^((ssh|sk-ssh)-)/.test(pubkey)) {
    warn("Value from 1Password does not look like an SSH public key");
    return;
  }

  try {
    await $`git config --global user.signingKey ${pubkey} --replace-all`;
    log("Updated git user.signingKey from 1Password");
  } catch (e) {
    warn("Failed to set git user.signingKey");
  }
}

async function isOnePasswordSignedIn(): Promise<boolean> {
  const { userName } = getUserContext();

  const whoamiResult =
    await $`OP_ACCOUNT=my.1password.com sudo -u ${userName} -E op whoami --format json`.nothrow();

  if (whoamiResult.exitCode === 1) {
    const loginResult =
      await $`OP_ACCOUNT=my.1password.com sudo -u ${userName} -E op signin`.nothrow();
    if (loginResult.exitCode === 0) {
      console.log("1Password login successful");
      return true;
    } else {
      console.log("1Password login failed", loginResult.text());
      return false;
    }
  }

  return true;
}

async function main() {
  try {
    // Restart Raycast independently
    // Configure Git signing from 1Password independently (best-effort)
    if (await isOnePasswordSignedIn()) {
      await configureGitSigningFrom1Password();
    }

    await restartRaycastApp();
  } catch (e) {
    err(String(e));
    process.exitCode = 1;
  }
}

main();

async function restartRaycastApp() {
  log("Attempting to quit Raycast...");
  // Send quit signal. Ignore error if it's not running.
  const quitOk = await retryAsync(
    5,
    () => $`pkill -x Raycast`.nothrow(),
    "Quit Raycast"
  );
  if (!quitOk) warn("Failed to send quit to Raycast after retries");

  log("Waiting for Raycast to exit completely...");
  const exited = await waitForProcessToDisappear("Raycast", 10_000);
  if (!exited) {
    warn("Raycast did not exit in time; forcing termination...");
    await $`pkill -9 -x Raycast`.nothrow();
    await waitForProcessToDisappear("Raycast", 5_000);
  }

  log("Restarting Raycast...");
  const opened = await $`open -a "Raycast"`.nothrow();
  if (opened.exitCode === 0) {
    log("Raycast has been restarted.");
  } else {
    warn(`Failed to restart Raycast: ${opened.text()}`);
  }
}
