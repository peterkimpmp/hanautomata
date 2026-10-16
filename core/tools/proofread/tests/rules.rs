use hanautomata_proofread::{Checker, Lexicon, Request};
struct Known;
impl Lexicon for Known {
    fn contains(&mut self, _: &str) -> bool { true }
    fn suggest(&mut self, _: &str) -> Vec<String> { vec![] }
    fn engine_id(&self) -> String { "test".into() }
    fn dictionary_id(&self) -> String { "test".into() }
}
fn request(text: &str) -> Request {
    Request { request_id:"r".into(),document_id:"d".into(),revision:"1".into(),text:text.into(),protected_words:vec![] }
}
#[test]
fn rules_are_opt_in_exact_and_preserve_protected_contexts() {
    let text = "되요 할수있다";
    assert!(Checker::new(Known).check(&request(text)).unwrap().diagnostics.is_empty());
    let mut checker = Checker::new(Known).with_rules(true);
    let report = checker.check(&request(text)).unwrap();
    assert_eq!(report.diagnostics.len(), 2);
    assert_eq!(report.diagnostics[0].suggestions, ["돼요"]);
    assert_eq!(report.diagnostics[1].suggestions, ["할 수 있다"]);
    for text in ["돼요 되어도 되다 되니", "할 수 있다 할수록", "`되요`", "https://example.org/되요", "되요라는이름"] {
        assert!(checker.check(&request(text)).unwrap().diagnostics.is_empty(), "{text}");
    }
    let mut protected = request("되요");
    protected.protected_words.push("되요".into());
    assert!(checker.check(&protected).unwrap().diagnostics.is_empty());
}
