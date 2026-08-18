use std::env;
use std::path::PathBuf;
use std::time::Instant;

use polyglot_vault::git::collect_rename_aliases;
use polyglot_vault::reconcile::snapshot;
use polyglot_vault::scan::scan_files;

fn main() {
    let root: PathBuf = env::args().nth(1).map(PathBuf::from).unwrap_or_else(|| ".".into());

    let files = scan_files(&root);
    println!("files: {}", files.len());

    let start = Instant::now();
    let snap = snapshot(&root).expect("reconcile scan failed");
    println!("reconcile scan: {} files in {:?}", snap.len(), start.elapsed());

    let start = Instant::now();
    let aliases = collect_rename_aliases(&root).expect("git rename collection failed");
    println!("rename aliases: {} in {:?}", aliases.len(), start.elapsed());
    for a in aliases.iter().take(3) {
        println!("  {} -> {} ({})", a.from, a.to, a.commit);
    }
}
