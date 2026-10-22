fn js_escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"") /* backslashes first,
    then quotes */
}
// one
// two
// three
