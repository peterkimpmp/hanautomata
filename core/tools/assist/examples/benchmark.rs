use hanautomata_assist::Library;
use std::{hint::black_box, time::Instant};
fn main() {
    let mut lib = Library::default();
    lib.set_enabled(true);
    for i in 0..512 {
        lib.add(&format!("검토 후 {i:03}번째 회신을 드리겠습니다.")).unwrap();
    }
    let mut times = Vec::new();
    for _ in 0..2000 {
        let start = Instant::now();
        black_box(lib.query(black_box("검토"), 20000));
        times.push(start.elapsed().as_micros());
    }
    times.sort();
    println!(
        "entries=512 queries=2000 engine_only_us p50={} p95={} max={}",
        times[999], times[1899], times[1999]
    );
}
