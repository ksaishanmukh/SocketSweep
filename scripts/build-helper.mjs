#!/usr/bin/env node
// ============================================================================
// build-helper.mjs — compile the on-phone app-info helper to a .dex
// ============================================================================
// android-helper/ is a few Java files run on the phone with app_process. They
// are compiled against the SDK's android.jar, then converted to Dalvik
// bytecode with d8, giving src-tauri/bin/socketsweep-helper.dex.
//
// Needs a JDK (17+) and an Android SDK with a platform (API 30+) and
// build-tools. The SDK is found through ANDROID_HOME / ANDROID_SDK_ROOT or the
// default install locations.
//
//   npm run build:helper
// ============================================================================

import { execFileSync } from "node:child_process";
import { existsSync, readdirSync } from "node:fs";
import { copyFile, mkdir, mkdtemp, rm } from "node:fs/promises";
import { homedir, tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = join(dirname(fileURLToPath(import.meta.url)), "..");
const SOURCE_DIR = join(ROOT, "android-helper/src");
const OUTPUT = join(ROOT, "src-tauri/bin/socketsweep-helper.dex");
/** Oldest platform whose android.jar declares everything the helper calls. */
const MIN_PLATFORM = 30;
/** Same floor as the daemon: the oldest Android the app supports. */
const MIN_API = 30;

class BuildError extends Error {}

const byVersionDesc = (a, b) =>
  b.localeCompare(a, undefined, { numeric: true, sensitivity: "base" });

function sdkRoots() {
  return [
    process.env.ANDROID_HOME,
    process.env.ANDROID_SDK_ROOT,
    process.env.LOCALAPPDATA && join(process.env.LOCALAPPDATA, "Android/Sdk"),
    join(homedir(), "Android/Sdk"),
    join(homedir(), "Library/Android/sdk"),
  ].filter((p) => p && existsSync(p));
}

/** Newest SDK platform (>= MIN_PLATFORM) that has an android.jar. */
function findAndroidJar(sdk) {
  const dir = join(sdk, "platforms");
  if (!existsSync(dir)) return null;
  const platforms = readdirSync(dir)
    .map((name) => ({ name, level: Number(/^android-(\d+)/.exec(name)?.[1]) }))
    .filter((p) => p.level >= MIN_PLATFORM && existsSync(join(dir, p.name, "android.jar")))
    .map((p) => p.name)
    .sort((a, b) => byVersionDesc(a.slice(8), b.slice(8)));
  return platforms.length ? join(dir, platforms[0], "android.jar") : null;
}

/**
 * Newest stable build-tools with d8. Its jar is run through java directly
 * rather than d8.bat / d8, which would need a shell on Windows.
 */
function findD8Jar(sdk) {
  const dir = join(sdk, "build-tools");
  if (!existsSync(dir)) return null;
  const versions = readdirSync(dir)
    .filter((v) => /^\d+\.\d+\.\d+$/.test(v) && existsSync(join(dir, v, "lib/d8.jar")))
    .sort(byVersionDesc);
  return versions.length ? join(dir, versions[0], "lib/d8.jar") : null;
}

function findSdk() {
  const roots = sdkRoots();
  if (!roots.length) {
    throw new BuildError(
      "Android SDK not found. Install it and set ANDROID_HOME (or ANDROID_SDK_ROOT).",
    );
  }
  for (const sdk of roots) {
    const androidJar = findAndroidJar(sdk);
    const d8Jar = findD8Jar(sdk);
    if (androidJar && d8Jar) return { androidJar, d8Jar };
  }
  throw new BuildError(
    `No usable Android SDK in ${roots.join(", ")}: it needs a platform with API ` +
      `${MIN_PLATFORM} or newer (platforms/android-*/android.jar) and stable build-tools ` +
      "with lib/d8.jar. Install them with sdkmanager.",
  );
}

/** Path to a JDK tool, preferring JAVA_HOME so it matches the JDK the user set up. */
function jdkTool(name) {
  const exe = process.platform === "win32" ? `${name}.exe` : name;
  const home = process.env.JAVA_HOME;
  return home && existsSync(join(home, "bin", exe)) ? join(home, "bin", exe) : name;
}

function run(tool, args) {
  try {
    execFileSync(tool, args, { stdio: ["ignore", "inherit", "inherit"] });
  } catch (e) {
    if (e.code === "ENOENT") {
      throw new BuildError(
        `${tool} not found. Install a JDK (17 or newer) and put it on PATH or set JAVA_HOME.`,
      );
    }
    throw new BuildError(`${tool} failed (exit ${e.status ?? "?"}).`);
  }
}

function filesWithExtension(dir, extension) {
  return readdirSync(dir, { withFileTypes: true }).flatMap((entry) => {
    const path = join(dir, entry.name);
    if (entry.isDirectory()) return filesWithExtension(path, extension);
    return entry.name.endsWith(extension) ? [path] : [];
  });
}

async function main() {
  const { androidJar, d8Jar } = findSdk();
  const work = await mkdtemp(join(tmpdir(), "socketsweep-helper-"));
  try {
    const classes = join(work, "classes");
    const dex = join(work, "dex");
    await mkdir(classes);
    await mkdir(dex);

    // --release 8 keeps the bytecode in what d8 desugars without extra
    // libraries; -Xlint:-options silences the "release 8 is obsolete" notice.
    run(jdkTool("javac"), [
      "--release",
      "8",
      "-Xlint:-options",
      "-encoding",
      "UTF-8",
      "-classpath",
      androidJar,
      "-d",
      classes,
      ...filesWithExtension(SOURCE_DIR, ".java"),
    ]);
    run(jdkTool("java"), [
      "-cp",
      d8Jar,
      "com.android.tools.r8.D8",
      "--release",
      "--min-api",
      String(MIN_API),
      "--lib",
      androidJar,
      "--output",
      dex,
      ...filesWithExtension(classes, ".class"),
    ]);

    await mkdir(dirname(OUTPUT), { recursive: true });
    await copyFile(join(dex, "classes.dex"), OUTPUT);
    console.log(`Built ${OUTPUT}`);
  } finally {
    await rm(work, { recursive: true, force: true });
  }
}

main().catch((e) => {
  console.error(e instanceof BuildError ? `build-helper: ${e.message}` : e);
  process.exit(1);
});
