//! Print the v4-vs-baseline comparison on the standard tree, and on the same
//! tree without its 60,000-file folder.
//!
//! `cargo run -p socketsweep-sim --example report --release [seed]`

use socketsweep_sim::{compare_on, format_report, SimConfig, SimFs};

fn main() {
    let seed = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(1);
    let cfg = SimConfig::standard();
    for (name, huge) in [
        ("standard tree", true),
        ("standard tree without the 60k folder", false),
    ] {
        let fs = SimFs::sdcard(seed, huge);
        let t = fs.truth(cfg.opts.max_depth, cfg.opts.one_filesystem).root();
        println!("== {name}: {} files, {} dirs", t.files, t.dirs);
        print!("{}", format_report(&compare_on(&fs, seed, &cfg)));
    }
}
