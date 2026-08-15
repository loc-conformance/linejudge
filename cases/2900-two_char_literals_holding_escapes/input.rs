fn js_escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}
// one
// two
// three
