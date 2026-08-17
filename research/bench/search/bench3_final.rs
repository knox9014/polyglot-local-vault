//! 가정 검증 #1-c — 병목 재규명 및 규모별 한계점 탐색
//!
//! bench2 결과로 기각된 것:
//!   H3 (incremental 축소) — 후보가 20만 개로 유지되어 축소 효과 없음
//!   H4 (greedy 스코어)    — top-10 일치율 25%, "router"가 router.js를 못 찾음. 품질 붕괴
//!
//! 남은 가설:
//!   H5. 진짜 병목은 DP가 아니라 부분수열 검사가 경로 바이트 전체를 훑는 것이다
//!   H6. DP span을 fzf처럼 backward pass로 조이면 품질 손실 없이 DP 비용이 사라진다
//!   H7. memchr(SIMD)로 다음 문자로 점프하면 부분수열 검사가 크게 빨라진다
//!
//! 그리고 가장 중요한 질문:
//!   Q. 16ms 예산을 만족하는 최대 vault 규모는 몇 개인가?

use memchr::memchr;
use std::fs;
use std::time::Instant;

const SCORE_MATCH: i32 = 16;
const BONUS_BOUNDARY: i32 = 8;
const BONUS_CAMEL: i32 = 7;
const BONUS_CONSEC: i32 = 8;
const PENALTY_GAP_START: i32 = -3;
const PENALTY_GAP_EXT: i32 = -1;
const BONUS_FILENAME: i32 = 4;

#[inline(always)]
fn is_boundary(p: u8) -> bool { matches!(p, b'/' | b'_' | b'-' | b'.' | b' ' | b'\\') }

struct Corpus {
    buf: Vec<u8>,
    lower: Vec<u8>,
    offs: Vec<u32>,
    base: Vec<u32>,
}

impl Corpus {
    fn load(path: &str, limit: usize) -> Corpus {
        let raw = fs::read_to_string(path).expect("corpus");
        let (mut buf, mut lower, mut base) = (Vec::new(), Vec::new(), Vec::new());
        let mut offs = vec![0u32];
        for line in raw.lines() {
            if line.is_empty() { continue; }
            if base.len() >= limit { break; }
            let start = buf.len() as u32;
            let mut last_slash = start;
            for &b in line.as_bytes() {
                buf.push(b);
                lower.push(b.to_ascii_lowercase());
                if b == b'/' { last_slash = buf.len() as u32; }
            }
            offs.push(buf.len() as u32);
            base.push(last_slash);
        }
        Corpus { buf, lower, offs, base }
    }
    #[inline(always)] fn n(&self) -> usize { self.base.len() }
    fn raw(&self, i: usize) -> &str {
        std::str::from_utf8(&self.buf[self.offs[i] as usize..self.offs[i + 1] as usize]).unwrap_or("?")
    }
}

/// 전진 패스: SIMD memchr로 다음 쿼리 문자로 점프. 실패하면 즉시 탈락.
#[inline(always)]
fn forward_end(hay: &[u8], needle: &[u8]) -> Option<usize> {
    let mut pos = 0usize;
    for &q in needle {
        match memchr(q, &hay[pos..]) {
            Some(off) => pos += off + 1,
            None => return None,
        }
    }
    Some(pos)
}

/// 후진 패스: end에서 거꾸로 매치해 가장 조인 start를 찾는다.
#[inline(always)]
fn backward_start(hay: &[u8], needle: &[u8], end: usize) -> usize {
    let mut qi = needle.len();
    let mut i = end;
    while i > 0 && qi > 0 {
        i -= 1;
        if hay[i] == needle[qi - 1] { qi -= 1; }
    }
    i
}

#[allow(clippy::too_many_arguments)]
fn score_dp(raw: &[u8], low: &[u8], needle: &[u8], s: usize, e: usize, fbase: usize, plen: usize,
            ps: &mut Vec<i32>, pd: &mut Vec<i32>, cs: &mut Vec<i32>, cd: &mut Vec<i32>) -> i32 {
    let n = e - s;
    let m = needle.len();
    ps.clear(); ps.resize(n + 1, 0);
    pd.clear(); pd.resize(n + 1, i32::MIN / 2);
    cs.clear(); cs.resize(n + 1, i32::MIN / 2);
    cd.clear(); cd.resize(n + 1, i32::MIN / 2);

    for qi in 0..m {
        let q = needle[qi];
        cs[0] = i32::MIN / 2; cd[0] = i32::MIN / 2;
        let mut in_gap = false;
        for hi in 0..n {
            let idx = s + hi;
            let gap = if in_gap { cs[hi] + PENALTY_GAP_EXT } else { cs[hi] + PENALTY_GAP_START };
            let mut bm = i32::MIN / 2;
            if low[idx] == q {
                let mut b = SCORE_MATCH;
                if idx == 0 || is_boundary(raw[idx - 1]) { b += BONUS_BOUNDARY; }
                else if raw[idx].is_ascii_uppercase() && raw[idx - 1].is_ascii_lowercase() { b += BONUS_CAMEL; }
                if idx >= fbase { b += BONUS_FILENAME; }
                let consec = if pd[hi] > i32::MIN / 4 { BONUS_CONSEC } else { 0 };
                bm = ps[hi].max(pd[hi]) + b + consec;
            }
            if bm >= gap { cs[hi + 1] = bm; cd[hi + 1] = bm; in_gap = false; }
            else { cs[hi + 1] = gap; cd[hi + 1] = i32::MIN / 2; in_gap = true; }
        }
        std::mem::swap(ps, cs);
        std::mem::swap(pd, cd);
    }
    let best = ps[1..].iter().copied().max().unwrap_or(i32::MIN);
    best - (plen as i32 / 16)
}

#[derive(Clone, Copy, Default)]
struct Prof { cands: usize, span_sum: usize }

/// 검색 범위 모드. 파일명만 볼 것인지 경로 전체를 볼 것인지.
#[derive(Clone, Copy, PartialEq)]
enum Scope { FullPath, FilenameOnly }

#[allow(clippy::too_many_arguments)]
fn search_range(c: &Corpus, needle: &[u8], lo: usize, hi: usize, k: usize, tight: bool, simd: bool,
                scope: Scope) -> (Vec<(i32, u32)>, Prof) {
    let mut out: Vec<(i32, u32)> = Vec::with_capacity(1024);
    let mut prof = Prof::default();
    let (mut ps, mut pd, mut cs, mut cd) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());

    for i in lo..hi {
        let off = if scope == Scope::FilenameOnly { c.base[i] as usize } else { c.offs[i] as usize };
        let endp = c.offs[i + 1] as usize;
        let hay = &c.lower[off..endp];

        let end_rel = if simd {
            match forward_end(hay, needle) { Some(e) => e, None => continue }
        } else {
            let mut qi = 0usize; let mut e = 0usize; let mut ok = false;
            for (j, &ch) in hay.iter().enumerate() {
                if ch == needle[qi] { qi += 1; if qi == needle.len() { e = j + 1; ok = true; break; } }
            }
            if !ok { continue }
            e
        };

        let start_rel = if tight {
            backward_start(hay, needle, end_rel)
        } else {
            match memchr(needle[0], hay) { Some(p) => p, None => continue }
        };

        prof.cands += 1;
        prof.span_sum += end_rel - start_rel;

        let sc = score_dp(&c.buf, &c.lower, needle, off + start_rel, off + end_rel,
                          c.base[i] as usize, endp - off, &mut ps, &mut pd, &mut cs, &mut cd);
        out.push((sc, i as u32));
    }
    if out.len() > k * 4 {
        out.select_nth_unstable_by(k, |a, b| b.0.cmp(&a.0));
        out.truncate(k);
    }
    (out, prof)
}

fn search(c: &Corpus, q: &str, k: usize, threads: usize, tight: bool, simd: bool, scope: Scope) -> (Vec<(i32, u32)>, Prof) {
    let needle: Vec<u8> = q.to_ascii_lowercase().into_bytes();
    let n = c.n();
    let (mut all, prof) = if threads <= 1 {
        search_range(c, &needle, 0, n, k, tight, simd, scope)
    } else {
        let chunk = n.div_ceil(threads);
        let parts: Vec<(Vec<(i32, u32)>, Prof)> = std::thread::scope(|s| {
            let mut hs = Vec::new();
            for t in 0..threads {
                let lo = t * chunk; let hi = ((t + 1) * chunk).min(n);
                if lo >= hi { continue; }
                let nd = &needle;
                hs.push(s.spawn(move || search_range(c, nd, lo, hi, k, tight, simd, scope)));
            }
            hs.into_iter().map(|h| h.join().unwrap()).collect()
        });
        let mut a = Vec::new(); let mut p = Prof::default();
        for (v, pp) in parts { a.extend(v); p.cands += pp.cands; p.span_sum += pp.span_sum; }
        (a, p)
    };
    if all.len() > k {
        all.select_nth_unstable_by(k, |a, b| b.0.cmp(&a.0));
        all.truncate(k);
    }
    all.sort_unstable_by(|a, b| b.0.cmp(&a.0));
    (all, prof)
}

fn pct(s: &[f64], p: f64) -> f64 { if s.is_empty() { 0.0 } else { s[(((s.len()-1) as f64)*p).round() as usize] } }

fn bench(c: &Corpus, q: &str, iters: usize, threads: usize, tight: bool, simd: bool, scope: Scope) -> (f64, f64, Prof) {
    for _ in 0..3 { std::hint::black_box(search(c, q, 50, threads, tight, simd, scope)); }
    let mut s = Vec::with_capacity(iters);
    let mut prof = Prof::default();
    for _ in 0..iters {
        let t0 = Instant::now();
        let r = search(c, q, 50, threads, tight, simd, scope);
        s.push(t0.elapsed().as_secs_f64() * 1e6);
        prof = r.1;
        std::hint::black_box(&r.0);
    }
    s.sort_by(|a, b| a.partial_cmp(b).unwrap());
    (pct(&s, 0.5), pct(&s, 0.95), prof)
}

const QUERIES: [&str; 8] = ["r", "ro", "rout", "router", "test", "testsr", "config", "tsconfig"];

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let path = a.get(1).map(|s| s.as_str()).unwrap_or("/tmp/corpus/shuffled.txt");
    let iters: usize = a.get(2).and_then(|s| s.parse().ok()).unwrap_or(20);
    let threads = std::thread::available_parallelism().map(|v| v.get()).unwrap_or(1);

    let c = Corpus::load(path, usize::MAX);
    println!("corpus = {} paths, {:.1} MB resident, threads = {}\n", c.n(),
        (c.buf.len() * 2 + c.offs.len() * 4 + c.base.len() * 4) as f64 / 1048576.0, threads);

    println!("### 최적화 기여도 분해 (232K, p95 us)");
    println!("{:<10} {:>11} {:>13} {:>13} {:>10} {:>8}",
        "query", "baseline", "+tight span", "+SIMD scan", "span avg", "gain");
    for q in QUERIES {
        let (_, b0, p0) = bench(&c, q, iters, threads, false, false, Scope::FullPath);
        let (_, b1, _) = bench(&c, q, iters, threads, true, false, Scope::FullPath);
        let (_, b2, p2) = bench(&c, q, iters, threads, true, true, Scope::FullPath);
        let span0 = if p0.cands > 0 { p0.span_sum as f64 / p0.cands as f64 } else { 0.0 };
        let span2 = if p2.cands > 0 { p2.span_sum as f64 / p2.cands as f64 } else { 0.0 };
        println!("{:<10} {:>11.0} {:>13.0} {:>13.0} {:>4.0}→{:>4.0} {:>7.1}x",
            q, b0, b1, b2, span0, span2, b0 / b2);
    }
    println!();

    println!("### 규모별 p95 (us).  16000us = 60fps 1프레임 예산");
    print!("{:<9}", "paths");
    for q in QUERIES { print!("{:>9}", q); }
    println!("{:>9}  verdict", "WORST");
    for size in [1_000usize, 5_000, 10_000, 25_000, 50_000, 100_000, 232_126] {
        if size > c.n() { continue; }
        let cs = Corpus::load(path, size);
        print!("{:<9}", size);
        let mut worst: f64 = 0.0;
        for q in QUERIES {
            let (_, p95, _) = bench(&cs, q, iters, threads, true, true, Scope::FullPath);
            worst = worst.max(p95);
            print!("{:>9.0}", p95);
        }
        println!("{:>9.0}  {}", worst, if worst <= 16000.0 { "OK" } else { "OVER" });
    }
    println!();

    println!("### 코어 스케일링 (232K, 최악 쿼리 \"testsr\")");
    for t in 1..=threads {
        let (p50, p95, _) = bench(&c, "testsr", iters, t, true, true, Scope::FullPath);
        println!("  threads={}  p50={:.0}us  p95={:.0}us", t, p50, p95);
    }
    println!();

    println!("### 결과 품질 (tight span DP — 정밀도 손실 없음)");
    for q in ["router", "tscfgjson", "k8sapisrv", "pkgjson"] {
        let (r, _) = search(&c, q, 3, threads, true, true, Scope::FullPath);
        println!("query = {:?}", q);
        for (s, i) in &r { println!("   {:>6}  {}", s, c.raw(*i as usize)); }
    }

    println!();
    println!("### 검색 범위 레버: 파일명만 vs 경로 전체 (p95 us)");
    println!("{:<10} {:>12} {:>14} {:>9}   {}", "query", "full path", "filename only", "gain", "top-1 (filename mode)");
    for q in QUERIES {
        let (_, full, _) = bench(&c, q, iters, threads, true, true, Scope::FullPath);
        let (_, fname, _) = bench(&c, q, iters, threads, true, true, Scope::FilenameOnly);
        let (r, _) = search(&c, q, 1, threads, true, true, Scope::FilenameOnly);
        let top = r.first().map(|(_, i)| c.raw(*i as usize)).unwrap_or("-");
        println!("{:<10} {:>12.0} {:>14.0} {:>8.1}x   {}", q, full, fname, full / fname, top);
    }
    println!();
    println!("### 규모별 p95 — 파일명 모드 (us)");
    print!("{:<9}", "paths");
    for q in QUERIES { print!("{:>9}", q); }
    println!("{:>9}  verdict", "WORST");
    for size in [50_000usize, 100_000, 232_126] {
        if size > c.n() { continue; }
        let cs = Corpus::load(path, size);
        print!("{:<9}", size);
        let mut worst: f64 = 0.0;
        for q in QUERIES {
            let (_, p95, _) = bench(&cs, q, iters, threads, true, true, Scope::FilenameOnly);
            worst = worst.max(p95);
            print!("{:>9.0}", p95);
        }
        println!("{:>9.0}  {}", worst, if worst <= 16000.0 { "OK" } else { "OVER" });
    }
}
