// Warm-engine decide latency, measured exactly as tests/IntelligenceProbe.cs (and the golden extractor's --latency) do:
// 50 repeats per fixture row, then every prefix as it is typed, plus the 64- and 128-key worst cases.
//   cargo run --release --example latency -- ../tests/corpora/intelligence-v014.tsv
use hanautomata_core::Detector;
use std::time::Instant;

fn main() {
    let path = std::env::args().nth(1).expect("usage: latency <fixture.tsv>");
    let text = std::fs::read_to_string(&path).expect("fixture readable");
    let samples: Vec<String> = text
        .lines()
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(|line| line.split('\t').next().unwrap().to_string())
        .collect();
    let detector = Detector::new("C:\\Users\\Test사용자");
    for raw in &samples {
        detector.decide(raw);
    }
    let mut timings = Vec::new();
    let mut time = |raw: &str| {
        let start = Instant::now();
        std::hint::black_box(detector.decide(raw));
        timings.push(start.elapsed().as_secs_f64() * 1000.0);
    };
    for raw in &samples {
        for _ in 0..50 {
            time(raw);
        }
    }
    let worst = ["a".repeat(64), "r".repeat(128)];
    for value in samples.iter().chain(worst.iter()) {
        let chars: Vec<char> = value.chars().collect();
        for len in 1..=chars.len() {
            time(&chars[..len].iter().collect::<String>());
        }
    }
    timings.sort_by(f64::total_cmp);
    let n = timings.len();
    let mean = timings.iter().sum::<f64>() / n as f64;
    println!(
        "engine=Rust samples={n} mean={mean:.4}ms p50={:.4}ms p95={:.4}ms p99={:.4}ms max={:.4}ms",
        timings[n / 2],
        timings[n * 95 / 100],
        timings[n * 99 / 100],
        timings[n - 1]
    );
}
