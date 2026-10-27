//! Write `src/bindings.ts` from the contract. See
//! [`socketsweep_gui_contract::bindings`].

use std::process::ExitCode;

use socketsweep_gui_contract::bindings;

fn main() -> ExitCode {
    let text = match bindings::render() {
        Ok(text) => text,
        Err(e) => {
            eprintln!("cannot export the GUI contract to TypeScript: {e}");
            return ExitCode::FAILURE;
        }
    };
    let path = bindings::bindings_path();
    if let Err(e) = std::fs::write(&path, text) {
        eprintln!("cannot write {}: {e}", path.display());
        return ExitCode::FAILURE;
    }
    println!("wrote {}", path.display());
    ExitCode::SUCCESS
}
