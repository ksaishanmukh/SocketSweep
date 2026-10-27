//! Generating `src/bindings.ts`, the GUI's only view of the host.
//!
//! The file holds a TypeScript type for every type in this crate, a
//! `commands` object with one typed `invoke` wrapper per [`Commands`](crate::Commands)
//! method, and an `events` object with one typed `listen` wrapper per
//! [`Event`](crate::Event). Types are rendered by `specta-typescript` with
//! `specta-serde`, so they follow the serde attributes (camelCase fields,
//! string enums); the wrappers are rendered here from [`command_defs`] and
//! [`event_defs`].
//!
//! Regenerate it with:
//!
//! ```text
//! cargo run -p socketsweep-gui-contract --bin export-bindings
//! ```
//!
//! The crate's tests fail while the checked-in file differs from what
//! [`render`] produces, so CI catches a contract change whose bindings were
//! not regenerated. Setting `UPDATE_BINDINGS=1` makes that test rewrite the
//! file instead.

use std::borrow::Cow;
use std::fmt::Write as _;
use std::path::PathBuf;

use specta::datatype::DataType;
use specta::Types;
use specta_typescript::{Exporter, FrameworkExporter, Typescript};

use crate::{command_defs, event_defs, CommandDef, CommandError, EventDef};

/// The generated file's location in the repository.
pub fn bindings_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../src/bindings.ts")
}

const HEADER: &str = "\
// Generated from crates/gui-contract by
//   cargo run -p socketsweep-gui-contract --bin export-bindings
// Do not edit by hand: CI fails if this file differs from what the crate generates.";

/// The full text of `src/bindings.ts`.
pub fn render() -> Result<String, specta_typescript::Error> {
    let mut types = Types::default();
    let commands = command_defs(&mut types);
    let events = event_defs(&mut types);
    // Only types reachable from a command's signature are exported on their
    // own; the error every command rejects with appears in none.
    <CommandError as specta::Type>::definition(&mut types);

    let exporter: Exporter = Typescript::default().into();
    exporter
        .header(HEADER)
        .framework_prelude("")
        .framework_runtime(move |exp| runtime(&exp, &commands, &events).map(Cow::Owned))
        .export(&types, specta_serde::Format)
}

/// The imports, `commands` and `events`. The types follow them in the file.
fn runtime(
    exp: &FrameworkExporter<'_>,
    commands: &[CommandDef],
    events: &[EventDef],
) -> Result<String, specta_typescript::Error> {
    let mut s = String::new();
    s.push_str("import { invoke } from \"@tauri-apps/api/core\";\n");
    s.push_str("import { listen, type UnlistenFn } from \"@tauri-apps/api/event\";\n\n");

    s.push_str(
        "/**\n * Every host command. Each rejects with a `CommandError` when it fails.\n */\n",
    );
    s.push_str("export const commands = {\n");
    for command in commands {
        jsdoc(&mut s, command.docs);
        let required = required_args(&command.args);
        let mut params = Vec::new();
        for (i, (name, ty)) in command.args.iter().enumerate() {
            let optional = if i >= required { "?" } else { "" };
            params.push(format!("{}{optional}: {}", camel(name), exp.reference(ty)?));
        }
        let ret = match &command.ret {
            Some(ty) => exp.reference(ty)?,
            None => "void".into(),
        };
        let args = if command.args.is_empty() {
            String::new()
        } else {
            let names: Vec<String> = command.args.iter().map(|(n, _)| camel(n)).collect();
            format!(", {{ {} }}", names.join(", "))
        };
        let _ = writeln!(
            s,
            "  {}: ({}): Promise<{ret}> =>\n    invoke<{ret}>(\"{}\"{args}),",
            camel(command.name),
            params.join(", "),
            command.name,
        );
    }
    s.push_str("};\n\n");

    s.push_str(
        "/**\n * Every host event. Each resolves to a function that stops listening.\n */\n",
    );
    s.push_str("export const events = {\n");
    for event in events {
        let payload = exp.reference(&event.payload)?;
        let _ = writeln!(
            s,
            "  {}: (handler: (payload: {payload}) => void): Promise<UnlistenFn> =>\n    listen<{payload}>(\"{}\", (e) => handler(e.payload)),",
            camel(&event.name.replace('-', "_")),
            event.name,
        );
    }
    s.push_str("};");
    Ok(s)
}

/// How many leading arguments are required: trailing nullable ones may be
/// left out, and Tauri reads a missing argument as `None`.
fn required_args(args: &[(&'static str, DataType)]) -> usize {
    args.iter()
        .rposition(|(_, ty)| !matches!(ty, DataType::Nullable(_)))
        .map_or(0, |i| i + 1)
}

fn jsdoc(s: &mut String, docs: &str) {
    let lines: Vec<&str> = docs.lines().map(str::trim).collect();
    if lines.iter().all(|l| l.is_empty()) {
        return;
    }
    s.push_str("  /**\n");
    for line in lines {
        let line = line.replace("*/", "*\\/");
        if line.is_empty() {
            s.push_str("   *\n");
        } else {
            let _ = writeln!(s, "   * {line}");
        }
    }
    s.push_str("   */\n");
}

/// `list_devices` → `listDevices`.
fn camel(snake: &str) -> String {
    let mut out = String::with_capacity(snake.len());
    let mut upper = false;
    for c in snake.chars() {
        if c == '_' {
            upper = true;
        } else if upper {
            out.extend(c.to_uppercase());
            upper = false;
        } else {
            out.push(c);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_camel_cased_like_tauri_does() {
        assert_eq!(camel("list_devices"), "listDevices");
        assert_eq!(camel("copy_to_desktop"), "copyToDesktop");
        assert_eq!(camel("focus"), "focus");
    }

    #[test]
    fn only_trailing_nullable_arguments_are_optional() {
        let s = || DataType::Primitive(specta::datatype::Primitive::str);
        let n = || DataType::Nullable(Box::new(s()));
        assert_eq!(required_args(&[]), 0);
        assert_eq!(required_args(&[("a", n()), ("b", n())]), 0);
        assert_eq!(required_args(&[("a", s()), ("b", n())]), 1);
        assert_eq!(required_args(&[("a", n()), ("b", s())]), 2);
    }

    /// The checked-in `src/bindings.ts` is what this crate generates. Run with
    /// `UPDATE_BINDINGS=1` to rewrite it.
    #[test]
    fn checked_in_bindings_are_up_to_date() {
        let fresh = render().expect("the contract exports to TypeScript");
        let path = bindings_path();
        if std::env::var_os("UPDATE_BINDINGS").is_some() {
            std::fs::write(&path, &fresh).expect("write src/bindings.ts");
            return;
        }
        let current = std::fs::read_to_string(&path)
            .unwrap_or_default()
            .replace("\r\n", "\n");
        assert!(
            current == fresh,
            "src/bindings.ts is stale. Regenerate it with \
             `cargo run -p socketsweep-gui-contract --bin export-bindings`."
        );
    }
}
