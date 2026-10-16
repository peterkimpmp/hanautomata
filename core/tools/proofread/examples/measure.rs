//! Reproducible synthetic smoke/timing observation, not an accuracy benchmark.
use hanautomata_proofread::{Checker, HunspellLexicon, Request};
use std::{path::PathBuf, time::Instant};

fn main() {
    let dir = PathBuf::from(std::env::args_os().nth(1).expect("dictionary directory"));
    let t = Instant::now();
    let mut checker = Checker::new(HunspellLexicon::load_pinned(&dir).expect("pinned dictionary"));
    let load_ms = t.elapsed().as_secs_f64() * 1000.0;
    let cases = [
        "안녕하세요",
        "돼요",
        "됐다",
        "학교에",
        "먹었습니다",
        "되요",
        "됬다",
        "웬지",
        "할수있다",
        "개발됬다",
    ];
    let mut observations = vec![];
    for word in cases {
        let q = Request {
            request_id: "measure".into(),
            document_id: "synthetic".into(),
            revision: "1".into(),
            text: word.into(),
            protected_words: vec![],
        };
        let t = Instant::now();
        let report = checker.check(&q).unwrap();
        observations
            .push(serde_json::json!({"text":word,"ms":t.elapsed().as_secs_f64()*1000.0,"diagnostics":report.diagnostics}));
    }
    let text = "안녕하세요 학교에 먹었습니다 됬다 웬지 개발됬다. ".repeat(10);
    let q = Request {
        request_id: "timing".into(),
        document_id: "synthetic".into(),
        revision: "1".into(),
        text: text.clone(),
        protected_words: vec![],
    };
    let mut ms = Vec::new();
    for _ in 0..30 {
        let t = Instant::now();
        let r = checker.check(&q).unwrap();
        assert!(!r.limited);
        assert_eq!(r.checked_words, 60);
        assert_eq!(r.diagnostics.len(), 30);
        ms.push(t.elapsed().as_secs_f64() * 1000.0);
    }
    ms.sort_by(f64::total_cmp);
    println!("{}", serde_json::to_string_pretty(&serde_json::json!({
        "label":"synthetic observation; not general accuracy or device comparison",
        "load_ms":load_ms,"cases":observations,"warm":{"runs":30,"chars":text.chars().count(),"p50_ms":ms[14],"p95_ms":ms[28],"max_ms":ms[29]}
    })).unwrap());
}
