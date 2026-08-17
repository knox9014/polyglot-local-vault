//! Polyglot Local Vault — 가정 검증 #1
//! "파일명/경로 검색은 인덱스 없이 in-memory 선형 스캔으로 프레임 예산(16ms) 안에 끝난다"
//!
//! 실제 저장소(TypeScript / kubernetes / cpython / node / rust)에서 추출한
//! 232,126개 실경로에 대해 fzf 계열 퍼지 매칭을 측정한다.
//!
//! 의존성 없음(std only). 측정 대상 외 요소를 최소화하기 위함.

use std::env;
use std::fs;
use std::time::Instant;

// ---------------------------------------------------------------------------
// 코퍼스: 문자열을 개별 할당하지 않고 flat buffer + offset 테이블로 보관한다.
// 실제 구현에서도 이렇게 해야 캐시 지역성이 확보되고 100K 규모에서
// 할당자 오버헤드가 사라진다.
// ---------------------------------------------------------------------------
struct Corpus {
    buf: Vec<u8>,       // 모든 경로를 이어붙인 원본 (소문자 정규화본은 lower)
    lower: Vec<u8>,     // 대소문자 무시 매칭용
    offs: Vec<u32>,     // i번째 경로 = [offs[i], offs[i+1])
    masks: Vec<u64>,    // 경로에 등장하는 문자 집합의 64bit 비트마스크 (프리필터)
    base: Vec<u32>,     // 파일명 시작 오프셋(마지막 '/' 다음). 파일명 가중치용
}

/// 문자 → 0..63 버킷. a-z=0..25, 0-9=26..35, 나머지 기호는 36..63에 접음.
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

impl Corpus {
    fn load(path: &str) -> Corpus {
        let raw = fs::read_to_string(path).expect("코퍼스를 읽을 수 없음");
        let mut buf = Vec::with_capacity(raw.len());
        let mut lower = Vec::with_capacity(raw.len());
        let mut offs = Vec::with_capacity(256 * 1024);
        let mut masks = Vec::with_capacity(256 * 1024);
        let mut base = Vec::with_capacity(256 * 1024);

        offs.push(0u32);
        for line in raw.lines() {
            if line.is_empty() {
                continue;
            }
            let start = buf.len() as u32;
            let mut m: u64 = 0;
            let mut last_slash = start;
            for &b in line.as_bytes() {
                let lb = b.to_ascii_lowercase();
                buf.push(b);
                lower.push(lb);
                m |= 1u64 << bucket(lb);
                if b == b'/' {
                    last_slash = buf.len() as u32; // '/' 다음 위치
                }
            }
            offs.push(buf.len() as u32);
            masks.push(m);
            base.push(last_slash);
        }
        Corpus { buf, lower, offs, masks, base }
    }

    #[inline(always)]
    fn len(&self) -> usize {
        self.masks.len()
    }

    #[inline(always)]
    fn get_lower(&self, i: usize) -> &[u8] {
        &self.lower[self.offs[i] as usize..self.offs[i + 1] as usize]
    }

    #[inline(always)]
    fn get_raw(&self, i: usize) -> &str {
        std::str::from_utf8(&self.buf[self.offs[i] as usize..self.offs[i + 1] as usize])
            .unwrap_or("<non-utf8>")
    }
}

// ---------------------------------------------------------------------------
// 점수 규칙 (fzf 계열을 단순화하되 실제 비용 구조는 유지)
// ---------------------------------------------------------------------------
const SCORE_MATCH: i32 = 16;
const BONUS_BOUNDARY: i32 = 8; // 경로 구분자/구분기호 직후
const BONUS_CAMEL: i32 = 7; // camelCase 경계
const BONUS_CONSEC: i32 = 8; // 연속 매치
const PENALTY_GAP_START: i32 = -3;
const PENALTY_GAP_EXT: i32 = -1;
const BONUS_FILENAME: i32 = 4; // 파일명 영역 매치 가산

#[inline(always)]
fn is_boundary(prev: u8) -> bool {
    matches!(prev, b'/' | b'_' | b'-' | b'.' | b' ' | b'\\')
}

/// 1단계: 부분수열 존재 여부만 O(n)으로 확인. 대부분의 후보가 여기서 탈락한다.
#[inline(always)]
fn subsequence_span(hay: &[u8], needle: &[u8]) -> Option<(usize, usize)> {
    let mut qi = 0usize;
    let mut first = usize::MAX;
    for (i, &c) in hay.iter().enumerate() {
        if c == needle[qi] {
            if qi == 0 {
                first = i;
            }
            qi += 1;
            if qi == needle.len() {
                return Some((first, i + 1));
            }
        }
    }
    None
}

/// 2단계: 생존자에 대해서만 DP 스코어링. 비용 O(span × |query|).
/// 이게 진짜 비싼 경로이며, 프리필터의 가치를 결정한다.
fn score_dp(
    hay_raw: &[u8],
    hay_low: &[u8],
    needle: &[u8],
    span: (usize, usize),
    fname_base: usize,
    path_len: usize,
) -> i32 {
    let (s, e) = span;
    let n = e - s;
    let m = needle.len();

    // 두 줄만 유지하는 롤링 DP
    let mut prev_score = vec![i32::MIN / 2; n + 1];
    let mut prev_diag = vec![i32::MIN / 2; n + 1];
    let mut cur_score = vec![i32::MIN / 2; n + 1];
    let mut cur_diag = vec![i32::MIN / 2; n + 1];

    for v in prev_score.iter_mut() {
        *v = 0;
    }

    for qi in 0..m {
        let q = needle[qi];
        cur_score[0] = i32::MIN / 2;
        cur_diag[0] = i32::MIN / 2;
        let mut in_gap = false;
        for hi in 0..n {
            let idx = s + hi;
            let c = hay_low[idx];

            // gap 경로
            let gap = if in_gap {
                cur_score[hi] + PENALTY_GAP_EXT
            } else {
                cur_score[hi] + PENALTY_GAP_START
            };

            let mut best_match = i32::MIN / 2;
            if c == q {
                let mut b = SCORE_MATCH;
                if idx == 0 || is_boundary(hay_raw[idx - 1]) {
                    b += BONUS_BOUNDARY;
                } else if hay_raw[idx].is_ascii_uppercase()
                    && hay_raw[idx - 1].is_ascii_lowercase()
                {
                    b += BONUS_CAMEL;
                }
                if idx >= fname_base {
                    b += BONUS_FILENAME;
                }
                // 이전 쿼리 문자가 바로 앞에서 매치되었으면 연속 보너스
                let consec = if prev_diag[hi] > i32::MIN / 4 { BONUS_CONSEC } else { 0 };
                best_match = prev_score[hi].max(prev_diag[hi]) + b + consec;
            }

            if best_match >= gap {
                cur_score[hi + 1] = best_match;
                cur_diag[hi + 1] = best_match;
                in_gap = false;
            } else {
                cur_score[hi + 1] = gap;
                cur_diag[hi + 1] = i32::MIN / 2;
                in_gap = true;
            }
        }
        std::mem::swap(&mut prev_score, &mut cur_score);
        std::mem::swap(&mut prev_diag, &mut cur_diag);
    }

    let mut best = i32::MIN;
    for v in prev_score.iter().skip(1) {
        if *v > best {
            best = *v;
        }
    }
    // 짧은 경로 선호 (동점 시 타이브레이크)
    best - (path_len as i32 / 16)
}

// ---------------------------------------------------------------------------
// Top-K 선택: 전체 정렬 대신 bounded 삽입. K=50이면 사실상 비용 0.
// ---------------------------------------------------------------------------
struct TopK {
    k: usize,
    items: Vec<(i32, u32)>, // (score, index) — 오름차순 유지, [0]이 최소
}

impl TopK {
    fn new(k: usize) -> Self {
        TopK { k, items: Vec::with_capacity(k + 1) }
    }
    #[inline(always)]
    fn floor(&self) -> i32 {
        if self.items.len() < self.k {
            i32::MIN
        } else {
            self.items[0].0
        }
    }
    #[inline(always)]
    fn push(&mut self, score: i32, idx: u32) {
        if self.items.len() == self.k {
            if score <= self.items[0].0 {
                return;
            }
            self.items.remove(0);
        }
        let pos = self.items.partition_point(|&(s, _)| s < score);
        self.items.insert(pos, (score, idx));
    }
    fn merge(&mut self, other: &TopK) {
        for &(s, i) in &other.items {
            self.push(s, i);
        }
    }
}

// ---------------------------------------------------------------------------
// 검색 커널
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
struct Stats {
    scanned: usize,
    survived_mask: usize,
    survived_subseq: usize,
}

fn search_range(
    c: &Corpus,
    needle: &[u8],
    qmask: u64,
    lo: usize,
    hi: usize,
    k: usize,
    use_prefilter: bool,
) -> (TopK, Stats) {
    let mut top = TopK::new(k);
    let mut st = Stats { scanned: 0, survived_mask: 0, survived_subseq: 0 };

    for i in lo..hi {
        st.scanned += 1;
        // --- 프리필터: 쿼리 문자 집합이 경로 문자 집합에 포함되지 않으면 즉시 탈락
        if use_prefilter && (c.masks[i] & qmask) != qmask {
            continue;
        }
        st.survived_mask += 1;

        let hay_low = c.get_lower(i);
        let span = match subsequence_span(hay_low, needle) {
            Some(s) => s,
            None => continue,
        };
        st.survived_subseq += 1;

        let off = c.offs[i] as usize;
        let fname_base = c.base[i] as usize;
        // DP는 절대 오프셋 기준으로 동작하므로 전체 버퍼를 넘긴다
        let sc = score_dp(
            &c.buf,
            &c.lower,
            needle,
            (off + span.0, off + span.1),
            fname_base,
            hay_low.len(),
        );
        if sc > top.floor() {
            top.push(sc, i as u32);
        }
    }
    (top, st)
}

fn search(c: &Corpus, query: &str, k: usize, threads: usize, use_prefilter: bool) -> (TopK, Stats) {
    let needle: Vec<u8> = query.to_ascii_lowercase().into_bytes();
    let mut qmask = 0u64;
    for &b in &needle {
        qmask |= 1u64 << bucket(b);
    }
    let n = c.len();

    if threads <= 1 {
        return search_range(c, &needle, qmask, 0, n, k, use_prefilter);
    }

    let chunk = n.div_ceil(threads);
    let results: Vec<(TopK, Stats)> = std::thread::scope(|s| {
        let mut hs = Vec::new();
        for t in 0..threads {
            let lo = t * chunk;
            let hi = ((t + 1) * chunk).min(n);
            let nd = &needle;
            if lo >= hi {
                continue;
            }
            hs.push(s.spawn(move || search_range(c, nd, qmask, lo, hi, k, use_prefilter)));
        }
        hs.into_iter().map(|h| h.join().unwrap()).collect()
    });

    let mut top = TopK::new(k);
    let mut st = Stats { scanned: 0, survived_mask: 0, survived_subseq: 0 };
    for (t, s) in &results {
        top.merge(t);
        st.scanned += s.scanned;
        st.survived_mask += s.survived_mask;
        st.survived_subseq += s.survived_subseq;
    }
    (top, st)
}

// ---------------------------------------------------------------------------
// 측정
// ---------------------------------------------------------------------------
fn percentile(sorted_us: &[f64], p: f64) -> f64 {
    if sorted_us.is_empty() {
        return 0.0;
    }
    let idx = ((sorted_us.len() as f64 - 1.0) * p).round() as usize;
    sorted_us[idx]
}

fn bench(c: &Corpus, query: &str, iters: usize, threads: usize, prefilter: bool) -> (f64, f64, f64, Stats, usize) {
    // 워밍업
    let (_, st) = search(c, query, 50, threads, prefilter);
    for _ in 0..3 {
        let _ = search(c, query, 50, threads, prefilter);
    }
    let mut samples = Vec::with_capacity(iters);
    let mut hits = 0;
    for _ in 0..iters {
        let t0 = Instant::now();
        let (top, _) = search(c, query, 50, threads, prefilter);
        let el = t0.elapsed().as_secs_f64() * 1e6;
        hits = top.items.len();
        samples.push(el);
        std::hint::black_box(&top.items);
    }
    samples.sort_by(|a, b| a.partial_cmp(b).unwrap());
    (
        percentile(&samples, 0.50),
        percentile(&samples, 0.95),
        percentile(&samples, 0.99),
        st,
        hits,
    )
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let corpus_path = args.get(1).map(|s| s.as_str()).unwrap_or("/tmp/corpus/all_paths.txt");
    let limit: usize = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(usize::MAX);
    let iters: usize = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(50);

    let t0 = Instant::now();
    let mut c = Corpus::load(corpus_path);
    // 규모별 측정을 위해 앞에서 자른다
    if limit < c.len() {
        c.masks.truncate(limit);
        c.base.truncate(limit);
        c.offs.truncate(limit + 1);
        let cut = c.offs[limit] as usize;
        c.buf.truncate(cut);
        c.lower.truncate(cut);
    }
    let load_ms = t0.elapsed().as_secs_f64() * 1e3;

    let mem = c.buf.len() + c.lower.len() + c.offs.len() * 4 + c.masks.len() * 8 + c.base.len() * 4;

    println!("### CORPUS");
    println!("paths         : {}", c.len());
    println!("load          : {:.1} ms", load_ms);
    println!("in-memory     : {:.2} MB  (raw {:.2} + lower {:.2} + idx {:.2})",
        mem as f64 / 1048576.0,
        c.buf.len() as f64 / 1048576.0,
        c.lower.len() as f64 / 1048576.0,
        (c.offs.len() * 4 + c.masks.len() * 8 + c.base.len() * 4) as f64 / 1048576.0);
    println!();

    // 키스트로크 시뮬레이션: 한 글자씩 늘어나는 실제 입력 패턴
    let keystroke_sets: Vec<Vec<&str>> = vec![
        vec!["r", "ro", "rou", "rout", "route", "router"],
        vec!["t", "te", "tes", "test", "tests", "testsr"],
        vec!["c", "co", "con", "conf", "confi", "config"],
    ];

    let nthreads = std::thread::available_parallelism().map(|v| v.get()).unwrap_or(1);

    for tcount in [1usize, nthreads] {
        println!("### KEYSTROKE LATENCY  (threads={}, prefilter=ON, top-50)", tcount);
        println!("{:<10} {:>9} {:>9} {:>9} {:>10} {:>10} {:>8}",
            "query", "p50 us", "p95 us", "p99 us", "mask-pass", "subseq-hit", "results");
        for set in &keystroke_sets {
            for q in set {
                let (p50, p95, p99, st, hits) = bench(&c, q, iters, tcount, true);
                println!("{:<10} {:>9.0} {:>9.0} {:>9.0} {:>10} {:>10} {:>8}",
                    q, p50, p95, p99, st.survived_mask, st.survived_subseq, hits);
            }
            println!();
        }
        if tcount == 1 && nthreads == 1 {
            break;
        }
    }

    // 프리필터 유무 비교 — 최악 쿼리 기준
    println!("### PREFILTER EFFECT  (threads={})", nthreads);
    println!("{:<10} {:>12} {:>12} {:>8}", "query", "ON p95 us", "OFF p95 us", "speedup");
    for q in ["r", "router", "tsconfig", "zzq"] {
        let (_, on, _, _, _) = bench(&c, q, iters, nthreads, true);
        let (_, off, _, _, _) = bench(&c, q, iters, nthreads, false);
        println!("{:<10} {:>12.0} {:>12.0} {:>7.1}x", q, on, off, off / on);
    }
    println!();

    // 실제 결과 품질 확인 (퍼지가 의미 있는 결과를 내는지)
    println!("### SAMPLE RESULTS");
    for q in ["router", "tscfgjson", "k8sapisrv"] {
        let (top, _) = search(&c, q, 5, nthreads, true);
        println!("query = {:?}", q);
        for &(s, i) in top.items.iter().rev() {
            println!("   {:>6}  {}", s, c.get_raw(i as usize));
        }
        println!();
    }
}
