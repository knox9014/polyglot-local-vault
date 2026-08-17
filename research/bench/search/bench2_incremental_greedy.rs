//! 가정 검증 #1-b — v1에서 16ms 예산을 초과한 원인 분리 및 최적화 검증
//!
//! v1 결과: 232K 경로에서 키스트로크 p95 = 20~56ms (2코어). 예산 초과.
//!
//! 가설:
//!   H1. 비트마스크 프리필터는 짧고 흔한 쿼리에서 무력하다 (생존율 97%)
//!   H2. 진짜 비용은 생존자 전원에 대한 DP 스코어링이다
//!   H3. 키스트로크 검색은 단조 축소적이다 —
//!       "route"의 후보는 "rout"의 후보의 부분집합이므로 전체 재스캔이 불필요하다
//!   H4. 후보가 매우 많을 때는 정밀 랭킹이 무의미하므로 저비용 스코어로 충분하다
//!
//! H3가 맞다면 첫 글자만 전체 스캔이고 이후는 후보 집합 내부 축소다.

use std::fs;
use std::time::Instant;

const SCORE_MATCH: i32 = 16;
const BONUS_BOUNDARY: i32 = 8;
const BONUS_CAMEL: i32 = 7;
const BONUS_CONSEC: i32 = 8;
const PENALTY_GAP_START: i32 = -3;
const PENALTY_GAP_EXT: i32 = -1;
const BONUS_FILENAME: i32 = 4;

/// 후보가 이 수를 넘으면 정밀 DP 대신 저비용 그리디 스코어를 쓴다 (H4)
const DP_CANDIDATE_LIMIT: usize = 4096;

#[inline(always)]
fn bucket(c: u8) -> u32 {
    match c {
        b'a'..=b'z' => (c - b'a') as u32,
        b'0'..=b'9' => 26 + (c - b'0') as u32,
        b'/' => 36,
        b'.' => 37,
        b'_' => 38,
        b'-' => 39,
        _ => 40 + (c as u32 & 23),
    }
}

#[inline(always)]
fn is_boundary(prev: u8) -> bool {
    matches!(prev, b'/' | b'_' | b'-' | b'.' | b' ' | b'\\')
}

struct Corpus {
    buf: Vec<u8>,
    lower: Vec<u8>,
    offs: Vec<u32>,
    masks: Vec<u64>,
    base: Vec<u32>,
}

impl Corpus {
    fn load(path: &str, limit: usize) -> Corpus {
        let raw = fs::read_to_string(path).expect("corpus");
        let mut buf = Vec::new();
        let mut lower = Vec::new();
        let mut offs = vec![0u32];
        let mut masks = Vec::new();
        let mut base = Vec::new();
        for line in raw.lines() {
            if line.is_empty() {
                continue;
            }
            if masks.len() >= limit {
                break;
            }
            let start = buf.len() as u32;
            let mut m = 0u64;
            let mut last_slash = start;
            for &b in line.as_bytes() {
                let lb = b.to_ascii_lowercase();
                buf.push(b);
                lower.push(lb);
                m |= 1u64 << bucket(lb);
                if b == b'/' {
                    last_slash = buf.len() as u32;
                }
            }
            offs.push(buf.len() as u32);
            masks.push(m);
            base.push(last_slash);
        }
        Corpus { buf, lower, offs, masks, base }
    }
    #[inline(always)]
    fn n(&self) -> usize { self.masks.len() }
    #[inline(always)]
    fn lo(&self, i: usize) -> &[u8] { &self.lower[self.offs[i] as usize..self.offs[i + 1] as usize] }
    fn raw(&self, i: usize) -> &str {
        std::str::from_utf8(&self.buf[self.offs[i] as usize..self.offs[i + 1] as usize]).unwrap_or("?")
    }
}

#[inline(always)]
fn subseq_span(hay: &[u8], needle: &[u8]) -> Option<(u32, u32)> {
    let mut qi = 0usize;
    let mut first = 0u32;
    for (i, &c) in hay.iter().enumerate() {
        if c == needle[qi] {
            if qi == 0 { first = i as u32; }
            qi += 1;
            if qi == needle.len() { return Some((first, i as u32 + 1)); }
        }
    }
    None
}

/// 저비용 그리디 스코어: 단일 패스, DP 없음. 경계/연속/파일명 보너스만 반영.
#[inline(always)]
fn score_greedy(raw: &[u8], low: &[u8], off: usize, len: usize, needle: &[u8], fbase: usize) -> i32 {
    let mut qi = 0usize;
    let mut score = 0i32;
    let mut prev_match: i64 = -2;
    for hi in 0..len {
        let idx = off + hi;
        if low[idx] == needle[qi] {
            let mut b = SCORE_MATCH;
            if idx == 0 || is_boundary(raw[idx - 1]) {
                b += BONUS_BOUNDARY;
            } else if raw[idx].is_ascii_uppercase() && raw[idx - 1].is_ascii_lowercase() {
                b += BONUS_CAMEL;
            }
            if idx >= fbase { b += BONUS_FILENAME; }
            if prev_match == idx as i64 - 1 { b += BONUS_CONSEC; }
            else if prev_match >= 0 {
                let gap = idx as i64 - prev_match - 1;
                b += PENALTY_GAP_START + PENALTY_GAP_EXT * (gap.min(8) as i32 - 1);
            }
            score += b;
            prev_match = idx as i64;
            qi += 1;
            if qi == needle.len() { break; }
        }
    }
    score - (len as i32 / 16)
}

fn score_dp(raw: &[u8], low: &[u8], needle: &[u8], span: (usize, usize), fbase: usize, plen: usize) -> i32 {
    let (s, e) = span;
    let n = e - s;
    let m = needle.len();
    let mut prev_s = vec![0i32; n + 1];
    let mut prev_d = vec![i32::MIN / 2; n + 1];
    let mut cur_s = vec![i32::MIN / 2; n + 1];
    let mut cur_d = vec![i32::MIN / 2; n + 1];
    for qi in 0..m {
        let q = needle[qi];
        cur_s[0] = i32::MIN / 2;
        cur_d[0] = i32::MIN / 2;
        let mut in_gap = false;
        for hi in 0..n {
            let idx = s + hi;
            let gap = if in_gap { cur_s[hi] + PENALTY_GAP_EXT } else { cur_s[hi] + PENALTY_GAP_START };
            let mut bm = i32::MIN / 2;
            if low[idx] == q {
                let mut b = SCORE_MATCH;
                if idx == 0 || is_boundary(raw[idx - 1]) { b += BONUS_BOUNDARY; }
                else if raw[idx].is_ascii_uppercase() && raw[idx - 1].is_ascii_lowercase() { b += BONUS_CAMEL; }
                if idx >= fbase { b += BONUS_FILENAME; }
                let consec = if prev_d[hi] > i32::MIN / 4 { BONUS_CONSEC } else { 0 };
                bm = prev_s[hi].max(prev_d[hi]) + b + consec;
            }
            if bm >= gap { cur_s[hi + 1] = bm; cur_d[hi + 1] = bm; in_gap = false; }
            else { cur_s[hi + 1] = gap; cur_d[hi + 1] = i32::MIN / 2; in_gap = true; }
        }
        std::mem::swap(&mut prev_s, &mut cur_s);
        std::mem::swap(&mut prev_d, &mut cur_d);
    }
    let best = prev_s[1..].iter().copied().max().unwrap_or(i32::MIN);
    best - (plen as i32 / 16)
}

fn topk(mut v: Vec<(i32, u32)>, k: usize) -> Vec<(i32, u32)> {
    if v.len() > k {
        v.select_nth_unstable_by(k, |a, b| b.0.cmp(&a.0));
        v.truncate(k);
    }
    v.sort_unstable_by(|a, b| b.0.cmp(&a.0));
    v
}

/// 세션: 키스트로크 간 후보 집합을 유지한다 (H3)
struct Session {
    query: String,
    cands: Vec<u32>, // 부분수열 매치에 성공한 인덱스 (스코어 무관)
}

#[derive(Default, Clone, Copy)]
struct Step {
    scanned: usize,
    cands: usize,
    used_dp: bool,
}

impl Session {
    fn new() -> Self { Session { query: String::new(), cands: Vec::new() } }

    fn search(&mut self, c: &Corpus, q: &str, k: usize, threads: usize, incremental: bool)
        -> (Vec<(i32, u32)>, Step) {
        let needle: Vec<u8> = q.to_ascii_lowercase().into_bytes();
        let mut qmask = 0u64;
        for &b in &needle { qmask |= 1u64 << bucket(b); }

        // 후보 소스 결정: 확장 입력이면 이전 후보 집합만 재검사
        let extend = incremental && !self.query.is_empty() && q.starts_with(&self.query);
        let mut new_cands: Vec<u32> = Vec::new();
        let mut scanned;

        if extend {
            scanned = self.cands.len();
            let src = &self.cands;
            new_cands = if threads <= 1 {
                src.iter().copied().filter(|&i| {
                    subseq_span(c.lo(i as usize), &needle).is_some()
                }).collect()
            } else {
                let chunk = src.len().div_ceil(threads);
                std::thread::scope(|s| {
                    let mut hs = Vec::new();
                    for t in 0..threads {
                        let lo = t * chunk; let hi = ((t + 1) * chunk).min(src.len());
                        if lo >= hi { continue; }
                        let nd = &needle;
                        hs.push(s.spawn(move || {
                            src[lo..hi].iter().copied()
                                .filter(|&i| subseq_span(c.lo(i as usize), nd).is_some())
                                .collect::<Vec<u32>>()
                        }));
                    }
                    hs.into_iter().flat_map(|h| h.join().unwrap()).collect()
                })
            };
        } else {
            scanned = c.n();
            new_cands = if threads <= 1 {
                (0..c.n() as u32).filter(|&i| {
                    let i = i as usize;
                    (c.masks[i] & qmask) == qmask && subseq_span(c.lo(i), &needle).is_some()
                }).collect()
            } else {
                let n = c.n();
                let chunk = n.div_ceil(threads);
                std::thread::scope(|s| {
                    let mut hs = Vec::new();
                    for t in 0..threads {
                        let lo = t * chunk; let hi = ((t + 1) * chunk).min(n);
                        if lo >= hi { continue; }
                        let nd = &needle;
                        hs.push(s.spawn(move || {
                            (lo..hi).filter(|&i| {
                                (c.masks[i] & qmask) == qmask && subseq_span(c.lo(i), nd).is_some()
                            }).map(|i| i as u32).collect::<Vec<u32>>()
                        }));
                    }
                    hs.into_iter().flat_map(|h| h.join().unwrap()).collect()
                })
            };
        }
        let _ = &mut scanned;

        // 스코어링: 후보 수에 따라 정밀도 전환 (H4)
        let use_dp = new_cands.len() <= DP_CANDIDATE_LIMIT;
        let score_one = |i: u32| -> (i32, u32) {
            let iu = i as usize;
            let off = c.offs[iu] as usize;
            let plen = c.offs[iu + 1] as usize - off;
            let fb = c.base[iu] as usize;
            let sc = if use_dp {
                let sp = subseq_span(c.lo(iu), &needle).unwrap();
                score_dp(&c.buf, &c.lower, &needle, (off + sp.0 as usize, off + sp.1 as usize), fb, plen)
            } else {
                score_greedy(&c.buf, &c.lower, off, plen, &needle, fb)
            };
            (sc, i)
        };

        let scored: Vec<(i32, u32)> = if threads <= 1 || new_cands.len() < 2000 {
            new_cands.iter().map(|&i| score_one(i)).collect()
        } else {
            let chunk = new_cands.len().div_ceil(threads);
            let src = &new_cands;
            std::thread::scope(|s| {
                let mut hs = Vec::new();
                for t in 0..threads {
                    let lo = t * chunk; let hi = ((t + 1) * chunk).min(src.len());
                    if lo >= hi { continue; }
                    let nd = &needle;
                    hs.push(s.spawn(move || {
                        src[lo..hi].iter().map(|&i| {
                            let iu = i as usize;
                            let off = c.offs[iu] as usize;
                            let plen = c.offs[iu + 1] as usize - off;
                            let fb = c.base[iu] as usize;
                            let sc = if use_dp {
                                let sp = subseq_span(c.lo(iu), nd).unwrap();
                                score_dp(&c.buf, &c.lower, nd, (off + sp.0 as usize, off + sp.1 as usize), fb, plen)
                            } else {
                                score_greedy(&c.buf, &c.lower, off, plen, nd, fb)
                            };
                            (sc, i)
                        }).collect::<Vec<(i32, u32)>>()
                    }));
                }
                hs.into_iter().flat_map(|h| h.join().unwrap()).collect()
            })
        };

        let step = Step { scanned, cands: new_cands.len(), used_dp: use_dp };
        self.query = q.to_string();
        self.cands = new_cands;
        (topk(scored, k), step)
    }
}

fn pct(s: &[f64], p: f64) -> f64 {
    if s.is_empty() { return 0.0; }
    s[(((s.len() - 1) as f64) * p).round() as usize]
}

fn run_sequence(c: &Corpus, seq: &[&str], threads: usize, incremental: bool, iters: usize)
    -> Vec<(String, f64, f64, f64, Step)> {
    let mut out = Vec::new();
    for (si, q) in seq.iter().enumerate() {
        let mut samples = Vec::with_capacity(iters);
        let mut last = Step::default();
        for _ in 0..iters {
            // 매 반복마다 시퀀스를 처음부터 재생 → 세션 상태를 현실적으로 재현
            let mut sess = Session::new();
            for w in &seq[..si] {
                let _ = sess.search(c, w, 50, threads, incremental);
            }
            let t0 = Instant::now();
            let (r, st) = sess.search(c, q, 50, threads, incremental);
            samples.push(t0.elapsed().as_secs_f64() * 1e6);
            last = st;
            std::hint::black_box(&r);
        }
        samples.sort_by(|a, b| a.partial_cmp(b).unwrap());
        out.push((q.to_string(), pct(&samples, 0.5), pct(&samples, 0.95), pct(&samples, 0.99), last));
    }
    out
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let path = a.get(1).map(|s| s.as_str()).unwrap_or("/tmp/corpus/shuffled.txt");
    let limit: usize = a.get(2).and_then(|s| s.parse().ok()).unwrap_or(usize::MAX);
    let iters: usize = a.get(3).and_then(|s| s.parse().ok()).unwrap_or(20);

    let c = Corpus::load(path, limit);
    let threads = std::thread::available_parallelism().map(|v| v.get()).unwrap_or(1);
    println!("corpus = {} paths, threads = {}, iters = {}\n", c.n(), threads, iters);

    let seqs: Vec<Vec<&str>> = vec![
        vec!["r", "ro", "rou", "rout", "route", "router"],
        vec!["t", "te", "tes", "test", "tests", "testsr"],
        vec!["c", "co", "con", "conf", "confi", "config"],
    ];

    for seq in &seqs {
        println!("### 시퀀스: {}", seq.join(" → "));
        println!("{:<8} {:>26} {:>26}   {:>9} {:>6}", "", "[FULL RESCAN]", "[INCREMENTAL]", "", "");
        println!("{:<8} {:>8} {:>8} {:>8} {:>8} {:>8} {:>8}   {:>9} {:>6}",
            "query", "p50", "p95", "p99", "p50", "p95", "p99", "cands", "score");
        let full = run_sequence(&c, seq, threads, false, iters);
        let incr = run_sequence(&c, seq, threads, true, iters);
        for (f, i) in full.iter().zip(incr.iter()) {
            println!("{:<8} {:>8.0} {:>8.0} {:>8.0} {:>8.0} {:>8.0} {:>8.0}   {:>9} {:>6}",
                f.0, f.1, f.2, f.3, i.1, i.2, i.3, i.4.cands,
                if i.4.used_dp { "DP" } else { "greedy" });
        }
        println!();
    }

    // 품질 검증: greedy 스코어가 DP 대비 상위 결과를 얼마나 보존하는가
    println!("### 스코어링 정밀도 손실 (greedy vs DP, top-10 일치율)");
    let mut tot = 0usize; let mut agree = 0usize;
    for q in ["router", "config", "tsconfig", "testsr", "apiserver", "readme", "pkgjson", "srcindex"] {
        let mut s1 = Session::new();
        let (dp, _) = {
            // DP 강제: 후보를 제한 이하로 만들 수 없으므로 직접 계산
            let needle: Vec<u8> = q.to_ascii_lowercase().into_bytes();
            let mut v: Vec<(i32, u32)> = (0..c.n()).filter_map(|i| {
                let sp = subseq_span(c.lo(i), &needle)?;
                let off = c.offs[i] as usize;
                let plen = c.offs[i + 1] as usize - off;
                Some((score_dp(&c.buf, &c.lower, &needle,
                    (off + sp.0 as usize, off + sp.1 as usize), c.base[i] as usize, plen), i as u32))
            }).collect();
            v = topk(v, 10);
            (v, ())
        };
        let (gr, _) = {
            let needle: Vec<u8> = q.to_ascii_lowercase().into_bytes();
            let mut v: Vec<(i32, u32)> = (0..c.n()).filter_map(|i| {
                subseq_span(c.lo(i), &needle)?;
                let off = c.offs[i] as usize;
                let plen = c.offs[i + 1] as usize - off;
                Some((score_greedy(&c.buf, &c.lower, off, plen, &needle, c.base[i] as usize), i as u32))
            }).collect();
            v = topk(v, 10);
            (v, ())
        };
        let d: std::collections::HashSet<u32> = dp.iter().map(|x| x.1).collect();
        let g: std::collections::HashSet<u32> = gr.iter().map(|x| x.1).collect();
        let ov = d.intersection(&g).count();
        tot += 10; agree += ov;
        println!("  {:<12} top-10 overlap = {}/10", q, ov);
        let _ = &mut s1;
    }
    println!("  ─────────────────────────────");
    println!("  전체 일치율 = {:.0}%\n", agree as f64 / tot as f64 * 100.0);

    println!("### 최종 결과 샘플 (incremental + adaptive)");
    for q in ["router", "tscfgjson", "k8sapisrv", "vaultidx"] {
        let mut s = Session::new();
        let mut r = Vec::new();
        for l in 1..=q.len() {
            r = s.search(&c, &q[..l], 5, threads, true).0;
        }
        println!("query = {:?}", q);
        for (sc, i) in r.iter().take(3) {
            println!("   {:>6}  {}", sc, c.raw(*i as usize));
        }
    }
}
