#!/usr/bin/env node
// ============================================================================
// build-daemon.mjs — cross-compile the device daemon for Android arm64 and x86_64
// ============================================================================
// One invocation for every build host, local and CI alike.
//
// Deliberately not cargo-ndk: the only thing it would do here is set the linker,
// since the daemon has no C dependencies needing CC/AR, and installing it costs
// a couple of minutes on every cold CI run.
//
//   npm run build:daemon
// ============================================================================

import { execFileSync } from "node:child_process";
import { existsSync, readdirSync } from "node:fs";
import { copyFile, mkdir, stat } from "node:fs/promises";
import { homedir } from "node:os";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = join(dirname(fileURLToPath(import.meta.url)), "..");
/** Rust target -> file name under src-tauri/bin (host-core's DaemonBinaries). */
const TARGETS = {
  "aarch64-linux-android": "daemon-arm64",
  "x86_64-linux-android": "daemon-x86_64",
};
const PROFILE = "release-daemon";
/** Minimum supported Android API level. */
const API_LEVEL = 30;

const HOST_TAG = {
  win32: "windows-x86_64",
  linux: "linux-x86_64",
  darwin: "darwin-x86_64",
}[process.platform];

function findNdk() {
  for (const key of ["ANDROID_NDK_HOME", "ANDROID_NDK_LATEST_HOME", "ANDROID_NDK_ROOT"]) {
    const v = process.env[key];
    if (v && existsSync(join(v, "toolchains/llvm/prebuilt"))) return v;
  }

  // Fall back to the default SDK locations, newest NDK first.
  const sdkRoots = [
    process.env.ANDROID_HOME,
    process.env.ANDROID_SDK_ROOT,
    process.env.LOCALAPPDATA && join(process.env.LOCALAPPDATA, "Android/Sdk"),
    join(homedir(), "Android/Sdk"),
    join(homedir(), "Library/Android/sdk"),
  ].filter(Boolean);

  for (const sdk of sdkRoots) {
    const ndkDir = join(sdk, "ndk");
    if (!existsSync(ndkDir)) continue;
    const versions = readdirSync(ndkDir)
      .filter((v) => existsSync(join(ndkDir, v, "toolchains/llvm/prebuilt")))
      // Numeric-aware sort so 30.x beats 9.x.
      .sort((a, b) => b.localeCompare(a, undefined, { numeric: true, sensitivity: "base" }));
    if (versions.length) return join(ndkDir, versions[0]);
  }
  return null;
}

function ensureRustTarget(TARGET) {
  const installed = execFileSync("rustup", ["target", "list", "--installed"], {
    encoding: "utf8",
  });
  if (!installed.split(/\r?\n/).includes(TARGET)) {
    console.log(`Installing Rust target ${TARGET}…`);
    execFileSync("rustup", ["target", "add", TARGET], { stdio: "inherit" });
  }
}

async function main() {
  if (!HOST_TAG) {
    console.error(`Unsupported build host '${process.platform}'.`);
    process.exit(1);
  }

  const ndk = findNdk();
  if (!ndk) {
    console.error("Android NDK not found. Install it via Android Studio, or set ANDROID_NDK_HOME.");
    process.exit(1);
  }
  console.log(`NDK: ${ndk}`);

  const suffix = process.platform === "win32" ? ".cmd" : "";
  for (const [target, outName] of Object.entries(TARGETS)) {
    const linker = join(
      ndk,
      "toolchains/llvm/prebuilt",
      HOST_TAG,
      "bin",
      `${target}${API_LEVEL}-clang${suffix}`,
    );
    if (!existsSync(linker)) {
      console.error(`Linker not found at ${linker}`);
      process.exit(1);
    }

    ensureRustTarget(target);

    // Cargo reads the linker for a target from this env var, which is the only
    // Android-specific setup the daemon needs.
    const envKey = `CARGO_TARGET_${target.toUpperCase().replace(/-/g, "_")}_LINKER`;

    console.log(`Building ${target} (${PROFILE})�`);
    execFileSync(
      "cargo",
      ["build", "-p", "socketsweep-daemon", "--target", target, "--profile", PROFILE],
      { cwd: ROOT, stdio: "inherit", env: { ...process.env, [envKey]: linker } },
    );

    const built = join(ROOT, "target", target, PROFILE, "socketsweep-daemon");
    const dest = join(ROOT, "src-tauri", "bin", outName);
    await mkdir(dirname(dest), { recursive: true });
    await copyFile(built, dest);

    const { size } = await stat(dest);
    console.log(`Wrote ${dest} (${(size / 1024).toFixed(0)} KB)`);
  }
}

main().catch((err) => {
  console.error(err.message);
  process.exit(1);
});
