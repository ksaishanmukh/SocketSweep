#!/usr/bin/env node
// ============================================================================
// setup-bins.mjs — fetch host binaries that are deliberately not in git
// ============================================================================
// Downloads Google's platform-tools for the host OS and places `adb` (plus its
// companion DLLs on Windows) into src-tauri/bin/, where the app bundles them as
// resources.
//
//   npm run setup            # no-op if the binaries are already present
//   npm run setup -- --force # re-download
// ============================================================================

import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import { createWriteStream, existsSync, readFileSync } from "node:fs";
import { chmod, mkdir, mkdtemp, copyFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join, dirname } from "node:path";
import { Readable } from "node:stream";
import { pipeline } from "node:stream/promises";
import { fileURLToPath } from "node:url";

const ROOT = join(dirname(fileURLToPath(import.meta.url)), "..");
const BIN_DIR = join(ROOT, "src-tauri", "bin");

/**
 * Pinned platform-tools release. Bump the version and all three hashes together
 * (sha256 of the zip served at each URL below); the download is rejected if the
 * hash differs, so a moved or tampered archive never reaches the bundle.
 */
const PLATFORM_TOOLS_VERSION = "37.0.1";
const url = (os) =>
  `https://dl.google.com/android/repository/platform-tools_r${PLATFORM_TOOLS_VERSION}-${os}.zip`;

/** Per-platform download URL, expected SHA-256, and the files we lift out of the archive. */
const TARGETS = {
  win32: {
    url: url("win"),
    sha256: "45f4d63113e895ebde0c90f194099a4676b6ac653bd28d54314a9e022bbc1a99",
    files: ["adb.exe", "AdbWinApi.dll", "AdbWinUsbApi.dll"],
  },
  darwin: {
    url: url("darwin"),
    sha256: "ee39ad5967e95c2a07f04dbcbde96b1a0c916ba376096db5d2f498b7727a5d1d",
    files: ["adb"],
  },
  linux: {
    url: url("linux"),
    sha256: "d230f13842f60f782a8645f9c813f8f845bf36089ea7289f28c48f17979313f1",
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
        // Paths travel via the environment, never interpolated into the script,
        // so quotes or other metacharacters in them cannot break the command.
        "Expand-Archive -LiteralPath $env:SS_ZIP -DestinationPath $env:SS_DEST -Force",
      ],
      { stdio: "inherit", env: { ...process.env, SS_ZIP: zipPath, SS_DEST: destDir } },
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

    const actual = createHash("sha256").update(readFileSync(zipPath)).digest("hex");
    if (actual !== target.sha256) {
      throw new Error(
        `SHA-256 mismatch for ${target.url}
  expected ${target.sha256}
  actual   ${actual}`,
      );
    }

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
