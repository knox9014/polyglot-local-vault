use std::env;
use std::path::PathBuf;
use std::time::Instant;

use polyglot_vault::git::collect_rename_aliases;
use polyglot_vault::reconcile::snapshot;
use polyglot_vault::scan::scan_files;
use polyglot_vault::index::InvertedIndex;
use polyglot_vault::search::PathTable;

fn main() {
    let root: PathBuf = env::args().nth(1).map(PathBuf::from).unwrap_or_else(|| ".".into());

    let files = scan_files(&root);
    println!("files: {}", files.len());

    let start = Instant::now();
    let snap = snapshot(&root).expect("reconcile scan failed");
    println!("reconcile scan: {} files in {:?}", snap.len(), start.elapsed());

    let start = Instant::now();
    match collect_rename_aliases(&root) {
        Ok(aliases) => {
            println!("rename aliases: {} in {:?}", aliases.len(), start.elapsed());
            for a in aliases.iter().take(3) {
                println!("  {} -> {} ({})", a.from, a.to, a.commit);
            }
        }
        Err(e) => println!("rename aliases: skipped ({e})"),
    }

    let start = Instant::now();
    let table = PathTable::build(&root);
    println!(
        "path table: {} paths, {:.2} MB resident, built in {:?}",
        table.len(),
        table.resident_bytes() as f64 / 1_048_576.0,
        start.elapsed()
    );
    for query in ["router", "config", "test", "src/main"] {
        let mut latencies = Vec::with_capacity(20);
        let mut last = Vec::new();
        for _ in 0..20 {
            let t0 = Instant::now();
            last = table.search(query, 50);
            latencies.push(t0.elapsed());
        }
        latencies.sort();
        let p95 = latencies[(latencies.len() * 95 / 100).min(latencies.len() - 1)];
        println!(
            "  query {query:?}: {} hits, p95={p95:?}, top1={:?}",
            last.len(),
            last.first().map(|(_, i)| table.path(*i as usize))
        );
    }

    let start = Instant::now();
    let index = InvertedIndex::build(&root, &table);
    let elapsed = start.elapsed();
    let original_bytes: u64 = crate_dir_size(&root);
    let mb = |b: usize| b as f64 / 1_048_576.0;
    let b = index.size_breakdown();
    println!(
        "content index: {} docs indexed in {:?}, {:.2} MB total ({:.2}% of {:.1} MB original)",
        index.indexed_docs(),
        elapsed,
        mb(index.resident_bytes()),
        index.resident_bytes() as f64 / original_bytes.max(1) as f64 * 100.0,
        original_bytes as f64 / 1_048_576.0,
    );
    println!(
        "  postings {:.2} MB ({} entries) | term text {:.2} MB ({} unique) | doc term ids {:.2} MB | doc lens {:.2} MB | tags {:.2} MB",
        mb(b.postings),
        b.total_postings,
        mb(b.term_text),
        b.unique_terms,
        mb(b.doc_term_ids),
        mb(b.doc_lens),
        mb(b.tags),
    );
    for query in ["router", "config", "test"] {
        let hits = index.search(query, 5);
        println!(
            "  bm25 {query:?}: {} hits, top1={:?}",
            hits.len(),
            hits.first().map(|(_, i)| table.path(*i as usize))
        );
    }
}

fn crate_dir_size(root: &std::path::Path) -> u64 {
    scan_files(root).iter().filter_map(|p| std::fs::metadata(p).ok()).map(|m| m.len()).sum()
}
