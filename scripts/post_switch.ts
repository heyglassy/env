#!/usr/bin/env bun

import { existsSync } from "fs";
import { spawnSync } from "child_process";
import { $ } from "bun";

function log(msg: string) {
  console.log(`[post-switch] ${msg}`);
}
function warn(msg: string) {
  console.error(`[post-switch][WARN] ${msg}`);
}
function err(msg: string) {
  console.error(`[post-switch][ERROR] ${msg}`);
}

function which(cmd: string): string | null {
  const pathEnv = process.env.PATH || "";
  for (const p of pathEnv.split(":")) {
    const full = `${p}/${cmd}`;
    if (existsSync(full)) return full;
  }
  return null;
}

function run(
  cmd: string,
  args: string[],
  opts?: { env?: NodeJS.ProcessEnv; stdio?: any }
): { status: number; stdout: string; stderr: string } {
  const result = spawnSync(cmd, args, {
    encoding: "utf8",
    stdio: ["ignore", "pipe", "pipe"],
    ...opts,
  });
  return {
    status: result.status ?? 0,
    stdout: result.stdout || "",
    stderr: result.stderr || "",
  };
}

async function sleep(ms: number) {
  return new Promise((res) => setTimeout(res, ms));
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
  const uidRes = run("id", ["-u", userName]);
  const uid =
    uidRes.status === 0 ? parseInt(uidRes.stdout.trim(), 10) : undefined;
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

function runAsUser(
  cmd: string,
  args: string[],
  env: NodeJS.ProcessEnv,
  uid?: number
): { status: number; stdout: string; stderr: string } {
  const isRoot = (() => {
    try {
      return typeof process.getuid === "function" && process.getuid() === 0;
    } catch {
      return false;
    }
  })();
  if (isRoot && uid && !Number.isNaN(uid)) {
    return run("launchctl", ["asuser", String(uid), cmd, ...args], { env });
  }
  return run(cmd, args, { env });
}

// Raycast CLI verification removed

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
      await $`sudo -u carnegie -E op read "op://Personal/GitHub/public key"`.text()
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

async function main() {
  try {
    // Restart Raycast independently
    // await restartRaycastApp();
    // Configure Git signing from 1Password independently (best-effort)
    await configureGitSigningFrom1Password();
  } catch (e) {
    err(String(e));
    process.exitCode = 1;
  }
}

main();

async function restartRaycastApp() {
  const { env: userEnv, uid } = getUserContext();
  log("Attempting to quit Raycast...");
  // Send quit signal. Ignore error if it's not running.
  run("pkill", ["-x", "Raycast"]);

  log("Waiting for Raycast to exit completely...");
  for (;;) {
    const r = run("pgrep", ["-x", "Raycast"]);
    if (r.status !== 0) break;
    await sleep(100);
  }

  log("Restarting Raycast...");
  const opened = runAsUser("open", ["-a", "Raycast"], userEnv, uid);
  if (opened.status === 0) {
    log("Raycast has been restarted.");
  } else {
    warn(`Failed to restart Raycast: ${opened.stderr.trim()}`);
  }
}
