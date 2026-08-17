//! M6 — 전문검색 인덱스 규모 실측 (리뷰 §11 검증)
//!
//! 리뷰 §11에서 성능 목표로 이렇게 못박자고 했다.
//!     인덱스 크기 / 원본 크기 < 25%
//! 근거 없이 쓴 숫자였다. 실제 코퍼스로 역인덱스를 구축해 검증한다.
//!
//! 측정:
//!   - 원본 텍스트 바이트
//!   - 고유 term 수, posting 수
//!   - delta + varint 인코딩 후 인덱스 크기
//!   - 구축 시간 (cold indexing KPI의 근거)
//!   - term 빈도 분포 (stop-word 컷의 효과)

use std::collections::HashMap;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Instant;

const MAX_FILE: u64 = 1_048_576; // 1MB 초과는 본문 인덱싱 제외 (리뷰 §12 권고값)
const MIN_TOK: usize = 2;
const MAX_TOK: usize = 40;

fn is_skip_dir(name: &str) -> bool {
    matches!(name, ".git" | "node_modules" | "target" | "dist" | "build"
        | "__pycache__" | ".venv" | "venv")
}

fn collect(root: &Path, out: &mut Vec<PathBuf>) {
    let rd = match fs::read_dir(root) { Ok(r) => r, Err(_) => return };
    for e in rd.flatten() {
        let ft = match e.file_type() { Ok(f) => f, Err(_) => continue };
        let name = e.file_name();
        let name = name.to_string_lossy();
        if ft.is_dir() {
            if !is_skip_dir(&name) { collect(&e.path(), out); }
        } else if ft.is_file() {
            out.push(e.path());
        }
    }
}

/// 첫 8KB 안에 NUL 바이트가 있으면 바이너리로 판정 (리뷰 §12 권고 기준)
fn is_binary(buf: &[u8]) -> bool {
    buf.iter().take(8192).any(|&b| b == 0)
}

fn varint_len(mut v: u64) -> usize {
    let mut n = 1;
    while v >= 0x80 { v >>= 7; n += 1; }
    n
}

fn main() {
    let root = std::env::args().nth(1).unwrap_or_else(|| "/tmp/corpus".into());
    let root = Path::new(&root);

    let t_walk = Instant::now();
    let mut files = Vec::new();
    collect(root, &mut files);
    let walk_ms = t_walk.elapsed().as_secs_f64() * 1000.0;

    println!("### M6 — 전문검색 인덱스 규모\n");
    println!("대상: {}", root.display());
    println!("파일 열거: {} 개, {:.0} ms\n", files.len(), walk_ms);

    let t0 = Instant::now();
    // term -> (마지막 doc id, posting 수, delta 바이트 누계)
    let mut postings: HashMap<Vec<u8>, (u32, u32, usize)> = HashMap::with_capacity(1 << 20);
    let mut src_bytes: u64 = 0;
    let mut indexed = 0u32;
    let mut skipped_binary = 0u32;
    let mut skipped_large = 0u32;
    let mut total_tokens: u64 = 0;
    let mut buf = Vec::with_capacity(1 << 20);

    for (i, p) in files.iter().enumerate() {
        let md = match fs::metadata(p) { Ok(m) => m, Err(_) => continue };
        if md.len() > MAX_FILE { skipped_large += 1; continue; }
        buf.clear();
        let mut f = match fs::File::open(p) { Ok(f) => f, Err(_) => continue };
        if f.read_to_end(&mut buf).is_err() { continue; }
        if is_binary(&buf) { skipped_binary += 1; continue; }
        src_bytes += buf.len() as u64;
        let doc = i as u32;
        indexed += 1;

        // 토큰화: ascii 영숫자 + '_' , 소문자 정규화
        let mut tok: Vec<u8> = Vec::with_capacity(32);
        let mut flush = |tok: &mut Vec<u8>, postings: &mut HashMap<Vec<u8>, (u32, u32, usize)>,
                         total: &mut u64| {
            if tok.len() >= MIN_TOK && tok.len() <= MAX_TOK {
                *total += 1;
                match postings.get_mut(tok.as_slice()) {
                    Some(e) => {
                        if e.0 != doc {
                            let delta = (doc - e.0) as u64;
                            e.2 += varint_len(delta);
                            e.0 = doc;
                            e.1 += 1;
                        }
                    }
                    None => {
                        postings.insert(tok.clone(), (doc, 1, varint_len(doc as u64)));
                    }
                }
            }
            tok.clear();
        };
        for &b in &buf {
            if b.is_ascii_alphanumeric() || b == b'_' {
                tok.push(b.to_ascii_lowercase());
            } else {
                flush(&mut tok, &mut postings, &mut total_tokens);
            }
        }
        flush(&mut tok, &mut postings, &mut total_tokens);
    }
    let build_s = t0.elapsed().as_secs_f64();

    let n_terms = postings.len();
    let n_postings: u64 = postings.values().map(|v| v.1 as u64).sum();
    let posting_bytes: u64 = postings.values().map(|v| v.2 as u64).sum();
    let dict_bytes: u64 = postings.keys().map(|k| k.len() as u64 + 8).sum(); // term + 오프셋/길이
    let idx_bytes = posting_bytes + dict_bytes;

    println!("{:<28}{:>16}", "인덱싱된 파일", indexed);
    println!("{:<28}{:>16}  ({}는 바이너리, {}는 1MB 초과)", "제외된 파일",
        skipped_binary + skipped_large, skipped_binary, skipped_large);
    println!("{:<28}{:>13.1} MB", "원본 텍스트", src_bytes as f64 / 1048576.0);
    println!("{:<28}{:>16}", "총 토큰 출현", total_tokens);
    println!("{:<28}{:>16}", "고유 term", n_terms);
    println!("{:<28}{:>16}", "posting (term-doc 쌍)", n_postings);
    println!();
    println!("{:<28}{:>13.1} MB", "posting 리스트 (delta+varint)", posting_bytes as f64 / 1048576.0);
    println!("{:<28}{:>13.1} MB", "term dictionary", dict_bytes as f64 / 1048576.0);
    println!("{:<28}{:>13.1} MB", "인덱스 합계", idx_bytes as f64 / 1048576.0);
    println!();
    println!(">>> 인덱스 / 원본 = {:.1}%   (리뷰 §11 목표: < 25%)",
        idx_bytes as f64 / src_bytes as f64 * 100.0);
    println!(">>> 구축 시간     = {:.1} s   ({:.0} 파일/s, {:.1} MB/s)",
        build_s, indexed as f64 / build_s, src_bytes as f64 / 1048576.0 / build_s);
    println!();

    // stop-word 컷 효과: 상위 빈도 term 제거 시 절감
    let mut by_freq: Vec<u32> = postings.values().map(|v| v.1).collect();
    by_freq.sort_unstable_by(|a, b| b.cmp(a));
    println!("### 고빈도 term 컷 효과");
    println!("{:<20}{:>14}{:>14}", "상위 N개 제거", "제거 posting", "posting 절감");
    for n in [10usize, 100, 1000, 10000] {
        if n > by_freq.len() { break; }
        let cut: u64 = by_freq[..n].iter().map(|&v| v as u64).sum();
        println!("{:<20}{:>14}{:>13.1}%", n, cut, cut as f64 / n_postings as f64 * 100.0);
    }
    println!();

    // 규모별 외삽
    println!("### 규모별 외삽 (실측 {} 파일 기준)", indexed);
    println!("{:<14}{:>14}{:>16}", "파일 수", "인덱스 크기", "cold 구축 시간");
    for size in [1_000u32, 10_000, 50_000, 100_000, indexed] {
        let r = size as f64 / indexed as f64;
        println!("{:<14}{:>11.1} MB{:>13.1} s", size,
            idx_bytes as f64 / 1048576.0 * r, build_s * r as f64);
    }
}
