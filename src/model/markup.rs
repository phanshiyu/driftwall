//! A minimal scanner for the start tags of HTML and Svelte files, see "The markup scanner" in
//! `docs/DESIGN.md` for exactly what it reads and what it does not.
//!
//! It is shared by the crates that need to decide what a markup file holds: ownership asks whether
//! an HTML file has a `<script` element, and import extraction reads the script elements and the
//! `<link>` elements. It is not a parser: it finds start tags, their attributes and the text of
//! `<script>` elements, and nothing else.

/// A start tag of one of the wanted elements.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tag {
    /// The element name in ASCII lower case.
    pub name: String,
    /// The attributes in source order, names in ASCII lower case.
    pub attributes: Vec<Attribute>,
    /// The byte offset of the `<`.
    pub start: usize,
    /// The 1-indexed line of the `<`.
    pub line: u32,
    /// The text of a `<script>` element: from the end of its start tag to its end tag, or to the
    /// end of the file. `None` for every other element and for a tag written `<script ... />`.
    pub text: Option<Text>,
}

impl Tag {
    /// The value of the first attribute called `name` (lower case), `""` when it has no value.
    #[must_use]
    pub fn attribute(&self, name: &str) -> Option<&str> {
        self.attributes
            .iter()
            .find(|attribute| attribute.name == name)
            .map(|attribute| attribute.value.as_deref().unwrap_or(""))
    }
}

/// One attribute of a start tag.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attribute {
    /// The name in ASCII lower case.
    pub name: String,
    /// The value without its quotes, or `None` when the attribute is written without `=`.
    pub value: Option<String>,
}

/// A stretch of the file, as the byte offsets and the points tree-sitter needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Text {
    /// Where it starts.
    pub start: Position,
    /// Where it ends, exclusive.
    pub end: Position,
}

/// A place in the file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Position {
    /// The byte offset.
    pub byte: usize,
    /// The 0-indexed line.
    pub row: usize,
    /// The 0-indexed byte offset inside the line.
    pub column: usize,
}

/// The [`Position`] of the byte offset `byte` in `source`. An offset past the end is the end.
#[must_use]
pub fn position(source: &str, byte: usize) -> Position {
    let before = source.as_bytes().get(..byte).unwrap_or(source.as_bytes());
    let row = before.split(|&b| b == b'\n').count().saturating_sub(1);
    let line_start = before
        .iter()
        .rposition(|&b| b == b'\n')
        .map_or(0, |newline| newline + 1);
    Position {
        byte: before.len(),
        row,
        column: before.len() - line_start,
    }
}

/// Whether `source` holds a `<script` element.
#[must_use]
pub fn has_script(source: &str) -> bool {
    !scan(source, false, &["script"]).is_empty()
}

/// Every start tag in `source` whose lower case name is one of `wanted`, in source order.
///
/// With `braces`, an attribute value may be a Svelte `{ ... }` expression. Elements are found
/// wherever they are written. Comments, declarations, end tags and the content of `<script>` and
/// `<style>` elements are never read as tags.
#[must_use]
pub fn scan(source: &str, braces: bool, wanted: &[&str]) -> Vec<Tag> {
    let bytes = source.as_bytes();
    let mut tags = Vec::new();
    let mut at = 0;
    while let Some(lt) = find(bytes, at, b'<') {
        at = lt + 1;
        match bytes.get(at) {
            Some(b'!') => at = after_declaration(bytes, lt),
            Some(b'?' | b'/') => at = after(bytes, at, b'>'),
            Some(next) if next.is_ascii_alphabetic() => {
                let Some(open) = open_tag(bytes, lt, braces) else {
                    break;
                };
                at = open.end;
                let raw = matches!(open.name.as_str(), "script" | "style") && !open.self_closing;
                let mut text = None;
                if raw {
                    let close = find_end_tag(bytes, open.end, &open.name).unwrap_or(bytes.len());
                    if open.name == "script" {
                        text = Some(Text {
                            start: position(source, open.end),
                            end: position(source, close),
                        });
                    }
                    at = after(bytes, close, b'>');
                }
                if wanted.contains(&open.name.as_str()) {
                    tags.push(Tag {
                        name: open.name,
                        attributes: open.attributes,
                        start: lt,
                        line: u32::try_from(position(source, lt).row)
                            .map_or(u32::MAX, |row| row.saturating_add(1)),
                        text,
                    });
                }
            }
            _ => {}
        }
    }
    tags
}

/// A start tag as written.
struct Open {
    name: String,
    attributes: Vec<Attribute>,
    /// The offset just after the closing `>`.
    end: usize,
    self_closing: bool,
}

const fn is_space(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t' | b'\n' | b'\r' | 0x0c)
}

fn find(bytes: &[u8], from: usize, needle: u8) -> Option<usize> {
    let rest = bytes.get(from..)?;
    rest.iter().position(|&b| b == needle).map(|i| from + i)
}

/// The offset just after the next `needle` at or after `from`, or the end of the file.
fn after(bytes: &[u8], from: usize, needle: u8) -> usize {
    find(bytes, from, needle).map_or(bytes.len(), |at| at + 1)
}

/// The offset after the comment or declaration that starts with the `<` at `lt`.
fn after_declaration(bytes: &[u8], lt: usize) -> usize {
    let comment = bytes
        .get(lt..)
        .is_some_and(|rest| rest.starts_with(b"<!--"));
    if !comment {
        return after(bytes, lt, b'>');
    }
    let body = lt + 4;
    bytes
        .get(body..)
        .and_then(|rest| rest.windows(3).position(|window| window == b"-->"))
        .map_or(bytes.len(), |end| body + end + 3)
}

/// The offset of the `<` of the first end tag `</name` at or after `from`, where `name` is
/// followed by whitespace, `/`, `>` or the end of the file.
fn find_end_tag(bytes: &[u8], from: usize, name: &str) -> Option<usize> {
    let mut at = from;
    while let Some(lt) = find(bytes, at, b'<') {
        at = lt + 1;
        if bytes.get(at) != Some(&b'/') {
            continue;
        }
        let start = at + 1;
        let end = start + name.len();
        let named = bytes
            .get(start..end)
            .is_some_and(|word| word.eq_ignore_ascii_case(name.as_bytes()));
        let ends = bytes
            .get(end)
            .is_none_or(|&next| is_space(next) || next == b'/' || next == b'>');
        if named && ends {
            return Some(lt);
        }
    }
    None
}

/// Reads the start tag whose `<` is at `lt`. `None` when it has no closing `>`.
fn open_tag(bytes: &[u8], lt: usize, braces: bool) -> Option<Open> {
    let name_start = lt + 1;
    let mut at = name_start;
    while bytes
        .get(at)
        .is_some_and(|&b| !is_space(b) && b != b'/' && b != b'>')
    {
        at += 1;
    }
    let name = lower(bytes, name_start, at);
    let mut attributes = Vec::new();
    loop {
        while bytes.get(at).is_some_and(|&b| is_space(b)) {
            at += 1;
        }
        match bytes.get(at)? {
            b'>' => {
                return Some(Open {
                    name,
                    attributes,
                    end: at + 1,
                    self_closing: false,
                });
            }
            b'/' => {
                if bytes.get(at + 1) == Some(&b'>') {
                    return Some(Open {
                        name,
                        attributes,
                        end: at + 2,
                        self_closing: true,
                    });
                }
                at += 1;
                continue;
            }
            _ => {}
        }
        let attribute_start = at;
        // why: a leading `=` belongs to the name, as in HTML, and guarantees progress.
        at += 1;
        while bytes
            .get(at)
            .is_some_and(|&b| !is_space(b) && !matches!(b, b'=' | b'/' | b'>'))
        {
            at += 1;
        }
        let attribute_name = lower(bytes, attribute_start, at);
        let mut probe = at;
        while bytes.get(probe).is_some_and(|&b| is_space(b)) {
            probe += 1;
        }
        let value = if bytes.get(probe) == Some(&b'=') {
            probe += 1;
            while bytes.get(probe).is_some_and(|&b| is_space(b)) {
                probe += 1;
            }
            let (value, end) = attribute_value(bytes, probe, braces)?;
            at = end;
            Some(value)
        } else {
            None
        };
        attributes.push(Attribute {
            name: attribute_name,
            value,
        });
    }
}

/// The value starting at `at` and the offset after it. `None` when a quote or brace is never
/// closed.
fn attribute_value(bytes: &[u8], at: usize, braces: bool) -> Option<(String, usize)> {
    match bytes.get(at) {
        Some(&quote @ (b'"' | b'\'')) => {
            let close = find(bytes, at + 1, quote)?;
            Some((text(bytes, at + 1, close), close + 1))
        }
        Some(b'{') if braces => {
            let close = matching_brace(bytes, at)?;
            Some((text(bytes, at, close + 1), close + 1))
        }
        _ => {
            let mut end = at;
            while let Some(&b) = bytes.get(end) {
                let self_closing = b == b'/' && bytes.get(end + 1) == Some(&b'>');
                if is_space(b) || b == b'>' || self_closing {
                    break;
                }
                end += 1;
            }
            Some((text(bytes, at, end), end))
        }
    }
}

/// The offset of the `}` that closes the `{` at `open`, skipping quoted strings and nested braces.
fn matching_brace(bytes: &[u8], open: usize) -> Option<usize> {
    let mut depth = 0_usize;
    let mut at = open;
    while let Some(&b) = bytes.get(at) {
        match b {
            b'{' => depth += 1,
            b'}' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return Some(at);
                }
            }
            b'"' | b'\'' | b'`' => at = find(bytes, at + 1, b)?,
            _ => {}
        }
        at += 1;
    }
    None
}

fn text(bytes: &[u8], from: usize, to: usize) -> String {
    bytes
        .get(from..to)
        .map(|slice| String::from_utf8_lossy(slice).into_owned())
        .unwrap_or_default()
}

fn lower(bytes: &[u8], from: usize, to: usize) -> String {
    text(bytes, from, to).to_ascii_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scripts(source: &str) -> Vec<Tag> {
        scan(source, false, &["script"])
    }

    fn body<'a>(source: &'a str, tag: &Tag) -> &'a str {
        let text = tag.text.expect("script text");
        source
            .get(text.start.byte..text.end.byte)
            .expect("in range")
    }

    #[test]
    fn attributes_are_read_in_any_order_and_quoting() {
        let source = "<script defer type='module' SRC=\"/a.js\" data-x=y async>x</script>";
        let tags = scripts(source);
        assert_eq!(tags.len(), 1);
        let tag = &tags[0];
        assert_eq!(tag.attribute("type"), Some("module"));
        assert_eq!(tag.attribute("src"), Some("/a.js"));
        assert_eq!(tag.attribute("data-x"), Some("y"));
        assert_eq!(tag.attribute("defer"), Some(""));
        assert_eq!(tag.attribute("async"), Some(""));
        assert_eq!(tag.attribute("missing"), None);
        assert_eq!(body(source, tag), "x");
    }

    #[test]
    fn a_quoted_value_may_contain_a_closing_angle_and_span_lines() {
        let source = "<script data-a=\"a > b\n c\" src=x.js>1</script>";
        let tags = scripts(source);
        assert_eq!(tags[0].attribute("data-a"), Some("a > b\n c"));
        assert_eq!(tags[0].attribute("src"), Some("x.js"));
    }

    #[test]
    fn a_self_closing_script_has_no_text_and_does_not_swallow_the_rest() {
        let source = "<script src=\"a.js\" />\n<script>b</script>";
        let tags = scripts(source);
        assert_eq!(tags.len(), 2);
        assert_eq!(tags[0].text, None);
        assert_eq!(tags[0].attribute("src"), Some("a.js"));
        assert_eq!(body(source, &tags[1]), "b");
        assert_eq!(tags[1].line, 2);
    }

    #[test]
    fn an_unquoted_value_stops_before_a_self_closing_slash() {
        let tags = scripts("<script src=a.js/>");
        assert_eq!(tags[0].attribute("src"), Some("a.js"));
        assert_eq!(tags[0].text, None);
    }

    #[test]
    fn comments_hide_their_tags() {
        let source = "<!-- <script>a</script> -->\n<script>b</script><!-- <script>";
        let tags = scripts(source);
        assert_eq!(tags.len(), 1);
        assert_eq!(body(source, &tags[0]), "b");
        assert_eq!(tags[0].line, 2);
    }

    #[test]
    fn declarations_and_end_tags_are_skipped() {
        let source = "<!doctype html><?xml a='<script>'?></p><script>c</script>";
        assert_eq!(scripts(source).len(), 1);
    }

    #[test]
    fn script_text_ends_at_the_first_matching_end_tag_in_any_case() {
        let source = "<SCRIPT>a = '<b>'; if (a</script >b) {}</SCRIPT\n><script>z</script>";
        let tags = scripts(source);
        assert_eq!(tags.len(), 2);
        assert_eq!(body(source, &tags[0]), "a = '<b>'; if (a");
        assert_eq!(body(source, &tags[1]), "z");
    }

    #[test]
    fn a_script_without_an_end_tag_runs_to_the_end_of_the_file() {
        let source = "<script>a";
        assert_eq!(body(source, &scripts(source)[0]), "a");
    }

    #[test]
    fn style_content_is_not_markup() {
        let source = "<style>a::before { content: '<script>' }</style><script>b</script>";
        let tags = scripts(source);
        assert_eq!(tags.len(), 1);
        assert_eq!(body(source, &tags[0]), "b");
    }

    #[test]
    fn a_lone_angle_bracket_is_text() {
        assert_eq!(scripts("1 < 2 <3 <script>a</script>").len(), 1);
    }

    #[test]
    fn crlf_files_count_the_same_lines_and_columns_as_lf_files() {
        let source = "<p>\r\n</p>\r\n<script>\r\nimport a from 'a';\r\n</script>";
        let tags = scripts(source);
        assert_eq!(tags[0].line, 3);
        let text = tags[0].text.expect("text");
        assert_eq!((text.start.row, text.start.column), (2, 8));
        assert_eq!((text.end.row, text.end.column), (4, 0));
    }

    #[test]
    fn only_the_wanted_elements_are_returned() {
        let source = "<a href=x><link rel=modulepreload href=\"/m.js\"><script>1</script>";
        let tags = scan(source, false, &["link"]);
        assert_eq!(tags.len(), 1);
        assert_eq!(tags[0].name, "link");
        assert_eq!(tags[0].attribute("href"), Some("/m.js"));
    }

    #[test]
    fn a_tag_that_never_closes_ends_the_scan() {
        assert_eq!(scripts("<script src='a.js'").len(), 0);
        assert_eq!(scripts("<script data-a='x>1</script>").len(), 0);
    }

    #[test]
    fn brace_values_are_read_whole_in_svelte() {
        let source = "<div on:click={() => run('}')} class=\"a\"><script lang=\"ts\">x</script>";
        let tags = scan(source, true, &["div", "script"]);
        assert_eq!(tags.len(), 2);
        assert_eq!(tags[0].attribute("on:click"), Some("{() => run('}')}"));
        assert_eq!(tags[0].attribute("class"), Some("a"));
        assert_eq!(tags[1].attribute("lang"), Some("ts"));
    }

    #[test]
    fn has_script_tells_markup_with_scripts_from_markup_without() {
        assert!(has_script("<html><script src=a.js></script></html>"));
        assert!(has_script("<SCRIPT>x</SCRIPT>"));
        assert!(!has_script("<p>use &lt;script&gt;</p>"));
        assert!(!has_script("<!-- <script>x</script> -->"));
        assert!(!has_script("<scripts></scripts><script-x></script-x>"));
        assert!(!has_script(""));
    }

    #[test]
    fn position_counts_lines_and_bytes() {
        let source = "ab\ncd\n";
        assert_eq!(
            position(source, 4),
            Position {
                byte: 4,
                row: 1,
                column: 1
            }
        );
        assert_eq!(position(source, 99).byte, source.len());
    }
}
