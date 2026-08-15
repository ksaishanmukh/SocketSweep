#!/usr/bin/env node
// ============================================================================
// setup-bins.mjs — fetch host binaries that are deliberately not in git
// ============================================================================
// Downloads Google's platform-tools for the host OS and places `adb` (plus its
// companion DLLs on Windows) into src-tauri/bin/, which tauri.<platform>.conf.json
// declares as bundle resources.
//
//   npm run setup            # no-op if the binaries are already present
//   npm run setup -- --force # re-download
// ============================================================================

import { execFileSync } from "node:child_process";
import { createWriteStream, existsSync } from "node:fs";
import { chmod, mkdir, mkdtemp, copyFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join, dirname } from "node:path";
import { Readable } from "node:stream";
import { pipeline } from "node:stream/promises";
import { fileURLToPath } from "node:url";

const ROOT = join(dirname(fileURLToPath(import.meta.url)), "..");
const BIN_DIR = join(ROOT, "src-tauri", "bin");

/** Per-platform download URL and the files we lift out of the archive. */
const TARGETS = {
  win32: {
    url: "https://dl.google.com/android/repository/platform-tools-latest-windows.zip",
    files: ["adb.exe", "AdbWinApi.dll", "AdbWinUsbApi.dll"],
  },
  darwin: {
    url: "https://dl.google.com/android/repository/platform-tools-latest-darwin.zip",
    files: ["adb"],
  },
  linux: {
    url: "https://dl.google.com/android/repository/platform-tools-latest-linux.zip",
    files: ["adb"],
  },
};

function unzip(zipPath, destDir) {
  if (process.platform === "win32") {
    execFileSync(
      "powershell",
      [
        "-NoProfile",
        "-NonInteractive",
        "-Command",
        `Expand-Archive -LiteralPath '${zipPath}' -DestinationPath '${destDir}' -Force`,
      ],
      { stdio: "inherit" },
    );
  } else {
    execFileSync("unzip", ["-qo", zipPath, "-d", destDir], { stdio: "inherit" });
  }
}

async function main() {
  const force = process.argv.includes("--force");
  const target = TARGETS[process.platform];

  if (!target) {
    console.error(
      `Unsupported platform '${process.platform}'. Supported: ${Object.keys(TARGETS).join(", ")}.`,
    );
    process.exit(1);
  }

  await mkdir(BIN_DIR, { recursive: true });

  const present = target.files.every((f) => existsSync(join(BIN_DIR, f)));
  if (present && !force) {
    console.log("adb already present in src-tauri/bin — nothing to do (--force to re-download).");
    return;
  }

  const work = await mkdtemp(join(tmpdir(), "socketsweep-setup-"));
  try {
    console.log(`Downloading ${target.url}`);
    const res = await fetch(target.url);
    if (!res.ok) {
      throw new Error(`Download failed: HTTP ${res.status} ${res.statusText}`);
    }

    const zipPath = join(work, "platform-tools.zip");
    await pipeline(Readable.fromWeb(res.body), createWriteStream(zipPath));

    console.log("Extracting…");
    unzip(zipPath, work);

    for (const file of target.files) {
      const from = join(work, "platform-tools", file);
      if (!existsSync(from)) {
        throw new Error(`Expected '${file}' in the archive but it was not there.`);
      }
      const to = join(BIN_DIR, file);
      await copyFile(from, to);
      if (process.platform !== "win32") await chmod(to, 0o755);
    }

    console.log(`Installed ${target.files.join(", ")} into src-tauri/bin`);
  } finally {
    await rm(work, { recursive: true, force: true });
  }
}

main().catch((err) => {
  console.error(err.message);
  process.exit(1);
});
