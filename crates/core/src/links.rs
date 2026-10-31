// SPDX-License-Identifier: AGPL-3.0-or-later

//! Wiki links in markdown: `[[Note]]`, `[[Note|label]]`, `[[Note#Heading]]`
//! and `[[Note#Heading|label]]`.
//!
//! Links inside fenced code blocks and inline code are ignored, so code that
//! happens to contain `[[` is never treated as a link or rewritten.

use std::ops::Range;

/// One wiki link found in a body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WikiLink {
    /// Byte range of the whole `[[...]]` in the body.
    pub span: Range<usize>,
    /// Title of the target note, trimmed.
    pub target: String,
    /// Heading inside the target, if any.
    pub heading: Option<String>,
    /// Text shown instead of the target, if any.
    pub alias: Option<String>,
}

impl WikiLink {
    /// The link written back as markdown with another target.
    fn with_target(&self, target: &str) -> String {
        let mut text = format!("[[{target}");
        if let Some(heading) = &self.heading {
            text.push('#');
            text.push_str(heading);
        }
        if let Some(alias) = &self.alias {
            text.push('|');
            text.push_str(alias);
        }
        text.push_str("]]");
        text
    }
}

/// Key used to match a link to a note title: trimmed and lowercased, so
/// `[[tomato plan]]` finds "Tomato plan".
pub fn title_key(title: &str) -> String {
    title.trim().to_lowercase()
}

/// Byte ranges of the body that are code (fenced blocks and inline spans).
fn code_ranges(body: &str) -> Vec<Range<usize>> {
    let mut ranges = Vec::new();
    let mut fence: Option<(usize, &str)> = None;
    let mut offset = 0;
    for line in body.split_inclusive('\n') {
        let trimmed = line.trim_start();
        let marker = ["```", "~~~"].into_iter().find(|m| trimmed.starts_with(m));
        match (fence, marker) {
            (None, Some(marker)) => fence = Some((offset, marker)),
            (Some((start, open)), Some(marker)) if marker == open => {
                ranges.push(start..offset + line.len());
                fence = None;
            }
            (None, None) => ranges.extend(inline_code(line, offset)),
            _ => {}
        }
        offset += line.len();
    }
    if let Some((start, _)) = fence {
        ranges.push(start..body.len());
    }
    ranges
}

/// Inline code spans of one line: a run of backticks up to the next run of
/// the same length, as in `CommonMark`.
fn inline_code(line: &str, offset: usize) -> Vec<Range<usize>> {
    let bytes = line.as_bytes();
    let mut ranges = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'`' {
            i += 1;
            continue;
        }
        let start = i;
        while i < bytes.len() && bytes[i] == b'`' {
            i += 1;
        }
        let run = i - start;
        let mut j = i;
        let mut closed = None;
        while j < bytes.len() {
            if bytes[j] == b'`' {
                let close = j;
                while j < bytes.len() && bytes[j] == b'`' {
                    j += 1;
                }
                if j - close == run {
                    closed = Some(j);
                    break;
                }
            } else {
                j += 1;
            }
        }
        if let Some(end) = closed {
            ranges.push(offset + start..offset + end);
            i = end;
        }
    }
    ranges
}

/// Every wiki link of a body, in order.
pub fn parse(body: &str) -> Vec<WikiLink> {
    let code = code_ranges(body);
    let in_code = |pos: usize| code.iter().any(|r| r.contains(&pos));
    let mut links = Vec::new();
    let mut from = 0;
    while let Some(found) = body[from..].find("[[") {
        let start = from + found;
        let Some(len) = body[start + 2..].find("]]") else {
            break;
        };
        let inner = &body[start + 2..start + 2 + len];
        let end = start + 2 + len + 2;
        from = start + 2;
        if inner.contains('\n') || inner.contains("[[") || in_code(start) {
            continue;
        }
        let (reference, alias) = match inner.split_once('|') {
            Some((reference, alias)) => (reference, Some(alias.trim().to_owned())),
            None => (inner, None),
        };
        let (target, heading) = match reference.split_once('#') {
            Some((target, heading)) => (target, Some(heading.trim().to_owned())),
            None => (reference, None),
        };
        let target = target.trim();
        if target.is_empty() {
            continue;
        }
        links.push(WikiLink {
            span: start..end,
            target: target.to_owned(),
            heading: heading.filter(|h| !h.is_empty()),
            alias: alias.filter(|a| !a.is_empty()),
        });
        from = end;
    }
    links
}

/// Points every link to `old` (any case) at `new`, keeping headings and
/// labels. Returns `None` when nothing changed.
pub fn retarget(body: &str, old: &str, new: &str) -> Option<String> {
    let old = title_key(old);
    let mut out = String::with_capacity(body.len());
    let mut last = 0;
    for link in parse(body)
        .into_iter()
        .filter(|l| title_key(&l.target) == old)
    {
        out.push_str(&body[last..link.span.start]);
        out.push_str(&link.with_target(new));
        last = link.span.end;
    }
    (last > 0).then(|| {
        out.push_str(&body[last..]);
        out
    })
}

/// ATX headings (`# Title`) outside code, for `[[Note#` completion.
pub fn headings(body: &str) -> Vec<String> {
    let code = code_ranges(body);
    let mut found = Vec::new();
    let mut offset = 0;
    for line in body.split_inclusive('\n') {
        let start = offset;
        offset += line.len();
        if code.iter().any(|r| r.contains(&start)) {
            continue;
        }
        let trimmed = line.trim_end();
        let level = trimmed.bytes().take_while(|b| *b == b'#').count();
        if (1..=6).contains(&level) && trimmed[level..].starts_with(' ') {
            found.push(
                trimmed[level..]
                    .trim()
                    .trim_end_matches('#')
                    .trim()
                    .to_owned(),
            );
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    fn targets(body: &str) -> Vec<(String, Option<String>, Option<String>)> {
        parse(body)
            .into_iter()
            .map(|l| (l.target, l.heading, l.alias))
            .collect()
    }

    #[test]
    fn parses_every_form() {
        assert_eq!(
            targets(
                "See [[Garden log]], [[Garden log|the log]], [[Plan#Week 1]] and [[Plan#Week 2|next]]."
            ),
            vec![
                ("Garden log".into(), None, None),
                ("Garden log".into(), None, Some("the log".into())),
                ("Plan".into(), Some("Week 1".into()), None),
                ("Plan".into(), Some("Week 2".into()), Some("next".into())),
            ]
        );
    }

    #[test]
    fn ignores_code_empty_and_multiline() {
        let body = "```\n[[in fence]]\n```\nuse `[[inline]]` here, ``a ` [[double]]`` too\n[[ ]] [[a\nb]] [[real]]";
        assert_eq!(targets(body), vec![("real".into(), None, None)]);
    }

    #[test]
    fn unclosed_fence_hides_the_rest() {
        assert!(parse("~~~\n[[x]]").is_empty());
    }

    #[test]
    fn retargets_any_case_and_keeps_label_and_heading() {
        let body = "[[Old]] [[old|label]] [[OLD#Part]] [[Other]] `[[Old]]`";
        assert_eq!(
            retarget(body, "old", "New name").unwrap(),
            "[[New name]] [[New name|label]] [[New name#Part]] [[Other]] `[[Old]]`"
        );
        assert_eq!(retarget("[[Other]]", "Old", "New"), None);
    }

    #[test]
    fn finds_headings_outside_code() {
        let body =
            "# Plan\ntext\n## Week 1 ##\n```\n# not a heading\n```\n#nospace\n####### too deep";
        assert_eq!(headings(body), vec!["Plan", "Week 1"]);
    }

    #[test]
    fn title_keys_ignore_case_and_spaces() {
        assert_eq!(title_key("  Tomato Plan "), "tomato plan");
    }
}
