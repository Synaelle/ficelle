// SPDX-License-Identifier: AGPL-3.0-or-later

//! Plain text preview of a markdown body, for note lists. Not a markdown
//! renderer: it only removes the syntax people would notice in a one line
//! summary (markers, link targets, images, escapes, footnote marks, folded
//! section tags) and turns emoji shortcodes into their emoji.

use std::collections::HashMap;
use std::sync::LazyLock;

/// GitHub's emoji shortcodes the editor knows, the same table.
static EMOJI: LazyLock<HashMap<String, String>> = LazyLock::new(|| {
    serde_json::from_str(include_str!("emoji.json")).expect("emoji.json is a name to emoji map")
});

/// The tags of a folded section, which read as nothing in a summary.
const DETAILS_TAGS: [&str; 5] = [
    "<details>",
    "<details open>",
    "</details>",
    "<summary>",
    "</summary>",
];

/// A footnote's label, `[^label]` without the brackets: letters, digits,
/// dashes and underscores. Its length with the brackets.
fn footnote(rest: &str) -> Option<usize> {
    let label = rest.strip_prefix("[^")?;
    let end = label.find(']')?;
    let ok = end > 0
        && label[..end]
            .chars()
            .all(|c| c.is_alphanumeric() || c == '-' || c == '_');
    ok.then_some(end + 3)
}

/// The emoji of a `:name:` shortcode at the start of `rest`, and its length.
fn shortcode(rest: &str) -> Option<(&'static str, usize)> {
    let name = rest.strip_prefix(':')?;
    let end = name.find(':')?;
    let emoji = EMOJI.get(&name[..end])?;
    Some((emoji.as_str(), end + 2))
}

/// Removes a list, quote, heading or task marker at the start of a line.
fn strip_line_start(mut line: &str) -> &str {
    loop {
        let trimmed = line.trim_start();
        let next = if let Some(rest) = trimmed.strip_prefix('>') {
            rest
        } else if trimmed.starts_with('#') {
            trimmed.trim_start_matches('#')
        } else if let Some(rest) = ["- ", "* ", "+ "]
            .iter()
            .find_map(|m| trimmed.strip_prefix(m))
        {
            rest
        } else if let Some(rest) = ["[ ] ", "[x] ", "[X] "]
            .iter()
            .find_map(|m| trimmed.strip_prefix(m))
        {
            rest
        } else if let Some(rest) = trimmed
            .strip_prefix("[!")
            .and_then(|r| r.split_once(']').map(|(_, rest)| rest))
        {
            rest
        } else if let Some(rest) =
            footnote(trimmed).and_then(|len| trimmed[len..].strip_prefix(':'))
        {
            // a footnote's text, its label left out
            rest
        } else {
            let digits = trimmed.bytes().take_while(u8::is_ascii_digit).count();
            match trimmed[digits..]
                .strip_prefix(". ")
                .or_else(|| trimmed[digits..].strip_prefix(") "))
            {
                Some(rest) if digits > 0 => rest,
                _ => return trimmed,
            }
        };
        line = next;
    }
}

/// Character for an html entity like `&gt;` or `&#62;`, and its length.
/// Older notes were saved with `&`, `<` and `>` written as entities.
fn entity(rest: &str) -> Option<(char, usize)> {
    let end = rest.get(..rest.len().min(12)).unwrap_or(rest).find(';')?;
    let name = &rest[1..end];
    let c = match name {
        "amp" => '&',
        "lt" => '<',
        "gt" => '>',
        "quot" => '"',
        "apos" => '\'',
        "nbsp" => '\u{a0}',
        _ => {
            let code = name.strip_prefix('#')?;
            let value = match code.strip_prefix(['x', 'X']) {
                Some(hex) => u32::from_str_radix(hex, 16).ok()?,
                None => code.parse().ok()?,
            };
            char::from_u32(value)?
        }
    };
    Some((c, end + 1))
}

/// Text of `[label](target)` or `[[target|label]]` links, images dropped,
/// emoji shortcodes as their emoji.
fn strip_inline(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut rest = line;
    while let Some(c) = rest.chars().next() {
        if let Some(escaped) = rest.strip_prefix('\\').and_then(|r| r.chars().next()) {
            out.push(escaped);
            rest = &rest[1 + escaped.len_utf8()..];
        } else if let Some(len) = footnote(rest) {
            rest = &rest[len..];
        } else if let Some(tag) = DETAILS_TAGS.iter().find(|t| rest.starts_with(**t)) {
            // the summary's words stay, apart from what follows
            out.push(' ');
            rest = &rest[tag.len()..];
        } else if let Some((emoji, len)) = shortcode(rest) {
            out.push_str(emoji);
            rest = &rest[len..];
        } else if rest.starts_with("[[") {
            if let Some(end) = rest.find("]]") {
                let inner = &rest[2..end];
                let label = inner
                    .rsplit_once('|')
                    .map_or_else(|| inner.split('#').next().unwrap_or(""), |(_, l)| l);
                out.push_str(label);
                rest = &rest[end + 2..];
            } else {
                out.push_str("[[");
                rest = &rest[2..];
            }
        } else if rest.starts_with("![") || rest.starts_with('[') {
            let image = rest.starts_with('!');
            let open = usize::from(image) + 1;
            let target = rest[open..].find("](").and_then(|mid| {
                rest[open + mid..]
                    .find(')')
                    .map(|close| (mid, open + mid + close))
            });
            if let Some((mid, close)) = target {
                if !image {
                    out.push_str(&rest[open..open + mid]);
                }
                rest = &rest[close + 1..];
                // layout block after an image: {width=320 wrap=left}
                if image && rest.starts_with('{') {
                    rest = rest.find('}').map_or(rest, |end| &rest[end + 1..]);
                }
            } else {
                out.push(c);
                rest = &rest[c.len_utf8()..];
            }
        } else if let Some((decoded, len)) = rest.starts_with('&').then(|| entity(rest)).flatten() {
            out.push(decoded);
            rest = &rest[len..];
        } else if matches!(c, '*' | '~' | '`') {
            rest = &rest[1..];
        } else {
            out.push(c);
            rest = &rest[c.len_utf8()..];
        }
    }
    out
}

/// A line that only draws something: a horizontal rule or a table's
/// separator row.
fn is_rule(line: &str) -> bool {
    let line = line.trim();
    !line.is_empty()
        && line
            .chars()
            .all(|c| matches!(c, '-' | '*' | '_' | '=' | '|' | ':' | ' '))
        && line
            .chars()
            .filter(|c| matches!(c, '-' | '*' | '_' | '='))
            .count()
            >= 3
}

/// A line holding only a link, a note link or an image (they read as
/// nothing on their own in a summary), or a bare web address.
fn is_link_only(line: &str) -> bool {
    let line = strip_line_start(line).trim();
    if line.starts_with("http://") || line.starts_with("https://") {
        return !line.contains(char::is_whitespace);
    }
    let bracketed = (line.starts_with("[[") && line.ends_with("]]"))
        || ((line.starts_with('[') || line.starts_with("!["))
            && (line.ends_with(')') || line.ends_with('}')));
    // one link only: no second link starting inside it
    bracketed && line[1..].matches("](").count() <= 1 && line[2..].matches("[[").count() == 0
}

/// Text lines of a body for its summary, code left out unless `with_code`.
fn text_lines(body: &str, with_code: bool) -> Vec<String> {
    let mut in_code = false;
    body.lines()
        .filter_map(|line| {
            let start = line.trim_start();
            if start.starts_with("```") || start.starts_with("~~~") {
                in_code = !in_code;
                return None;
            }
            if (in_code && !with_code) || is_rule(line) || (!in_code && is_link_only(line)) {
                return None;
            }
            let text = strip_inline(strip_line_start(line)).replace('|', " ");
            let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
            (!text.is_empty()).then_some(text)
        })
        .collect()
}

/// One line summary of a note, at most `max` characters. It starts with
/// real text: code, link or image only lines, rules and a first line that
/// repeats the title are left out. A note with nothing else shows its code.
pub fn preview(title: &str, body: &str, max: usize) -> String {
    let mut lines = text_lines(body, false);
    if lines.is_empty() {
        lines = text_lines(body, true);
    }
    let title = title.trim().to_lowercase();
    if !title.is_empty()
        && lines
            .first()
            .is_some_and(|first| first.to_lowercase() == title)
    {
        lines.remove(0);
    }
    let flat = lines.join(" ");
    match flat.char_indices().nth(max) {
        Some((cut, _)) => format!("{}…", &flat[..cut]),
        None => flat,
    }
}

#[cfg(test)]
mod tests {
    use super::preview;

    #[test]
    fn removes_markdown_people_would_notice() {
        let body = "# Plan\n\n![vines](attachments/a.png){width=320 wrap=left}Six **cedar** posts, see [[Garden log|the log]] and [site](https://x.y).\n\n- [ ] Ask *Inès*\n> [!note] Prune\n> weekly\n```\ncode = 1\n```\n1. \\# literal";
        assert_eq!(
            preview("", body, 200),
            "Plan Six cedar posts, see the log and site. Ask Inès Prune weekly # literal"
        );
    }

    #[test]
    fn keeps_snake_case_and_plain_brackets() {
        assert_eq!(
            preview("", "spacing_cm = 45 [draft] [[Note#Part]]", 100),
            "spacing_cm = 45 [draft] Note"
        );
    }

    #[test]
    fn decodes_entities_from_older_notes() {
        assert_eq!(
            preview(
                "",
                "a -&gt; b &lt;c&gt; R&amp;D &#233;t&#xE9; \\&amp; & lone &bogus;",
                100
            ),
            "a -> b <c> R&D été &amp; & lone &bogus;"
        );
    }

    #[test]
    fn shortens_long_text() {
        let long = "word ".repeat(100);
        let out = preview("", &long, 140);
        assert!(out.ends_with('…'));
        assert_eq!(out.chars().count(), 141);
    }

    #[test]
    fn skips_a_first_line_repeating_the_title() {
        assert_eq!(
            preview("Garden plan", "# Garden plan\n\nSix posts.", 100),
            "Six posts."
        );
        assert_eq!(preview("Café list", "café LIST\nBeans", 100), "Beans");
        // only the first line, and only when it is the title
        assert_eq!(preview("Plan", "Intro\n# Plan", 100), "Intro Plan");
        assert_eq!(preview("", "# Heading\nText", 100), "Heading Text");
    }

    #[test]
    fn skips_code_links_images_and_rules() {
        let body = "![photo](a.png){width=200}\nhttps://example.org/page\n[The site](https://x.y)\n[[Other note]]\n---\n```rust\nfn main() {}\n```\nThe real text.";
        assert_eq!(preview("", body, 100), "The real text.");
    }

    #[test]
    fn keeps_links_inside_sentences() {
        assert_eq!(
            preview("", "See [the site](https://x.y) and [[Note]] today.", 100),
            "See the site and Note today."
        );
        assert_eq!(preview("", "[a](x) and [b](y)", 100), "a and b");
    }

    #[test]
    fn tables_read_as_words() {
        assert_eq!(
            preview("", "| Plant | Days |\n| --- | :-: |\n| Basil | 7 |", 100),
            "Plant Days Basil 7"
        );
    }

    #[test]
    fn github_extras_read_as_their_text() {
        let body = "<details>\n<summary>Watering :seedling:</summary>\n\nTwice a week[^1], more in July :sunny:.\n\n</details>\n\n[^1]: Less when it rains.";
        assert_eq!(
            preview("", body, 200),
            "Watering 🌱 Twice a week, more in July ☀️. Less when it rains."
        );
        // an unknown name, times and plain brackets stay as written
        assert_eq!(
            preview("", "Meet at 10:30:00 :nope: [^] [^a b]", 100),
            "Meet at 10:30:00 :nope: [^] [^a b]"
        );
    }

    #[test]
    fn a_note_of_code_only_shows_its_code() {
        assert_eq!(preview("Snippet", "```\nlet x = 1\n```", 100), "let x = 1");
    }
}
