//! Classic catalog importer: classic/docs/classic/*.json → classic/assets/rules/classic/*.ron and
//! classic/docs/classic/implemented.md. Argument: repository root (default: two levels
//! above this crate).

fn main() {
    let root = match std::env::args().nth(1) {
        Some(path) => std::path::PathBuf::from(path),
        None => std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../.."),
    };
    match classic_content::import::run(&root) {
        Ok(summary) => {
            for file in &summary.files {
                println!("wrote {}", file.display());
            }
            println!("wrote {}", summary.report.display());
        }
        Err(error) => {
            eprintln!("classic-import: {error}");
            std::process::exit(1);
        }
    }
}
