//! Project-authored exact-token rules. No copied dictionary or consultation corpus.
//! These finite forms intentionally do not infer arbitrary morphology or author intent.
pub struct Rule {
    pub id: &'static str,
    pub replacement: &'static str,
    pub source: &'static str,
}
pub fn lookup(word: &str) -> Option<Rule> {
    let (id, replacement, source) = match word {
        "되요" => ("ko-doe-yo", "돼요", "https://korean.go.kr/front/mcfaq/mcfaqView.do?mcfaq_seq=6761&mn_id=62"),
        "할수있다" => ("ko-dependent-su", "할 수 있다", NORMS),
        "할수있어요" => ("ko-dependent-su", "할 수 있어요", NORMS),
        "할수없다" => ("ko-dependent-su", "할 수 없다", NORMS),
        "할수없어요" => ("ko-dependent-su", "할 수 없어요", NORMS),
        _ => return None,
    };
    Some(Rule { id, replacement, source })
}
const NORMS: &str = "https://www.korean.go.kr/kornorms/m/m_regltn.do"; // 한글 맞춤법 제42항
