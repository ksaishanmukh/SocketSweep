// Fails when a Rust source file declares more than MAX public items, so a file
// with a wide public surface gets split by idea instead of growing.
//
// Counted: `pub fn|struct|enum|trait|const|static|type|union` at any
// indentation (so public methods count too). Not counted: `pub(crate)` and
// other restricted forms, `pub mod` and `pub use` (they add no item of their
// own, like re-exports in the TS rule), anything inside a `#[cfg(test)]`
// block, and files under `tests/` or named `tests.rs`.
import { readdirSync, readFileSync } from "node:fs";
import { join, relative, sep } from "node:path";
import { fileURLToPath } from "node:url";

const MAX = 10;
const ROOTS = ["crates", "src-tauri/src"];

// Files that are one catalogue of types by design. Splitting them would only
// scatter a single vocabulary. Keys are repo-relative with forward slashes.
const ALLOWLIST = new Map([
  [
    "crates/wire/src/lib.rs",
    "the protocol's constants and id types: one vocabulary both ends share",
  ],
  [
    "crates/wire/src/messages/values.rs",
    "the value types messages carry (entry, stat, totals), all one layout",
  ],
  ["crates/gui-contract/src/nodes.rs", "the node, size and category types of the GUI contract"],
]);

const ITEM =
  /^\s*pub\s+(?:(?:const|async|unsafe|extern\s+"[^"]*")\s+)*(?:fn|struct|enum|trait|const|static|type|union)\b/;

/** Removes string and char literals and line comments so braces in them do not count. */
function stripNoise(line) {
  return line
    .replace(/"(?:[^"\\]|\\.)*"/g, '""')
    .replace(/'(?:[^'\\]|\\.)'/g, "''")
    .replace(/\/\/.*$/, "");
}

export function countPublicItems(source) {
  let count = 0;
  let pendingTestAttr = false;
  // Brace depth at which the open `#[cfg(test)]` block started, or null.
  let testDepth = null;
  let depth = 0;
  for (const raw of source.split(/\r?\n/)) {
    const line = stripNoise(raw);
    const trimmed = line.trim();
    if (testDepth === null) {
      if (trimmed.startsWith("#[cfg(test)]")) pendingTestAttr = true;
      else if (pendingTestAttr && trimmed !== "" && !trimmed.startsWith("#[")) {
        if (line.includes("{")) testDepth = depth;
        pendingTestAttr = false;
      } else if (ITEM.test(line)) count += 1;
    }
    for (const ch of line) {
      if (ch === "{") depth += 1;
      else if (ch === "}") depth -= 1;
    }
    if (testDepth !== null && depth <= testDepth) testDepth = null;
  }
  return count;
}

function* rustFiles(dir) {
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const path = join(dir, entry.name);
    if (entry.isDirectory()) {
      if (entry.name === "target" || entry.name === "tests") continue;
      yield* rustFiles(path);
    } else if (entry.name.endsWith(".rs") && entry.name !== "tests.rs") {
      yield path;
    }
  }
}

function main() {
  const root = fileURLToPath(new URL("..", import.meta.url));
  const failures = [];
  const used = new Set();
  for (const base of ROOTS) {
    for (const file of rustFiles(join(root, base))) {
      const rel = relative(root, file).split(sep).join("/");
      const count = countPublicItems(readFileSync(file, "utf8"));
      if (ALLOWLIST.has(rel)) {
        used.add(rel);
        if (count <= MAX) failures.push(`${rel}: ${count} items, allowlist entry no longer needed`);
      } else if (count > MAX) {
        failures.push(`${rel}: ${count} public items; the limit is ${MAX}. Split it by idea.`);
      }
    }
  }
  for (const rel of ALLOWLIST.keys()) {
    if (!used.has(rel)) failures.push(`${rel}: allowlisted but not found`);
  }
  if (failures.length > 0) {
    console.error(failures.join("\n"));
    process.exit(1);
  }
}

if (process.argv[1] === fileURLToPath(import.meta.url)) main();
