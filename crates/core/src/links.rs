// SPDX-License-Identifier: AGPL-3.0-or-later

//! Wiki links in markdown: `[[Note]]`, `[[Note|label]]`, `[[Note#Heading]]`
//! and `[[Note#Heading|label]]`.
//!
//! Links inside fenced code blocks and inline code are ignored, so code that
//! happens to contain `[[` is never treated as a link or rewritten.

use std::collections::HashMap;
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
    // the opening fence's character and length: as in CommonMark, only a
    // line of at least as many of the same closes it, so a longer fence
    // can hold a shorter one as code
    let mut fence: Option<(usize, char, usize)> = None;
    let mut offset = 0;
    for line in body.split_inclusive('\n') {
        let trimmed = line.trim_start();
        let marker = ['`', '~'].into_iter().find_map(|c| {
            let run = trimmed.len() - trimmed.trim_start_matches(c).len();
            (run >= 3).then_some((c, run))
        });
        match (fence, marker) {
            (None, Some((c, run))) => fence = Some((offset, c, run)),
            (Some((start, open, length)), Some((c, run)))
                if c == open && run >= length && trimmed[run..].trim().is_empty() =>
            {
                ranges.push(start..offset + line.len());
                fence = None;
            }
            (None, None) => ranges.extend(inline_code(line, offset)),
            _ => {}
        }
        offset += line.len();
    }
    if let Some((start, ..)) = fence {
        ranges.push(start..body.len());
    }
    ranges
}

/// Whether `pos` is inside one of `code`, which is sorted and disjoint.
fn in_code(code: &[Range<usize>], pos: usize) -> bool {
    let next = code.partition_point(|r| r.end <= pos);
    code.get(next).is_some_and(|r| r.contains(&pos))
}

/// Inline code spans of one line: a run of backticks up to the next run of
/// the same length, as in `CommonMark`.
fn inline_code(line: &str, offset: usize) -> Vec<Range<usize>> {
    let bytes = line.as_bytes();
    // where each run of backticks starts, by its length: the closing run is
    // looked up, a line of many runs is not read again for each of them
    let mut runs: HashMap<usize, Vec<usize>> = HashMap::new();
    let mut i = 0;
    while i < bytes.len() {
        let start = i;
        while i < bytes.len() && bytes[i] == b'`' {
            i += 1;
        }
        if i > start {
            runs.entry(i - start).or_default().push(start);
        } else {
            i += 1;
        }
    }
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
        let same = &runs[&(i - start)];
        if let Some(&close) = same.get(same.partition_point(|&at| at <= start)) {
            let end = close + (i - start);
            ranges.push(offset + start..offset + end);
            i = end;
        }
    }
    ranges
}

/// The end of the link opening with `[[` at `start`, past its `]]`. Without
/// one before a newline or another `[[`, `Err` with where to look next.
fn link_end(body: &str, start: usize) -> std::result::Result<usize, usize> {
    let bytes = body.as_bytes();
    let mut i = start + 2;
    while i < bytes.len() {
        match bytes[i] {
            b'\n' => return Err(i + 1),
            b']' if bytes.get(i + 1) == Some(&b']') => return Ok(i + 2),
            b'[' if bytes.get(i + 1) == Some(&b'[') => return Err(i),
            _ => i += 1,
        }
    }
    Err(bytes.len())
}

/// Every wiki link of a body, in order.
pub fn parse(body: &str) -> Vec<WikiLink> {
    let code = code_ranges(body);
    let mut links = Vec::new();
    let mut from = 0;
    while let Some(found) = body[from..].find("[[") {
        let start = from + found;
        let end = match link_end(body, start) {
            Ok(end) => end,
            Err(next) => {
                from = next;
                continue;
            }
        };
        let inner = &body[start + 2..end - 2];
        from = start + 2;
        if in_code(&code, start) {
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
        if in_code(&code, start) {
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
    fn a_longer_fence_holds_a_shorter_one() {
        let body = "````md\n```\n[[sample]]\n```\n````\n[[real]]\n```js\n``` x\n[[code]]\n```";
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

    /// Each `[[` once read up to a far `]]`: 80 000 of them took seconds on
    /// every save.
    #[test]
    fn many_openings_and_backtick_runs_are_read_in_one_pass() {
        let started = std::time::Instant::now();
        let many = format!("{}]] [[Plan]]", "[[".repeat(200_000));
        assert_eq!(targets(&many), vec![("Plan".into(), None, None)]);
        let labels = format!("{}]]\n[[Old]]", "[[a|".repeat(100_000));
        assert_eq!(
            retarget(&labels, "Old", "New").unwrap(),
            format!("{}]]\n[[New]]", "[[a|".repeat(100_000))
        );
        let runs: Vec<String> = (1..600).map(|n| "`".repeat(n)).collect();
        assert_eq!(targets(&format!("{} [[Plan]]", runs.join(" "))).len(), 1);
        assert!(started.elapsed().as_secs() < 2, "{:?}", started.elapsed());
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
