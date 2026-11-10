// SPDX-License-Identifier: AGPL-3.0-or-later

//! Full text search with SQLite FTS5.

use rusqlite::{params, params_from_iter};

use super::Store;
use crate::error::Result;
use crate::model::{MATCH_END, MATCH_START, SearchHit};

/// Turns what the user typed into an FTS5 query. Each word must appear,
/// as a whole word or as the start of one. Quotes and FTS5 operators typed by
/// the user are treated as plain text, so no input can break the query.
fn fts_query(input: &str) -> Option<String> {
    let terms: Vec<String> = input
        .split_whitespace()
        .map(|word| word.replace('"', ""))
        .filter(|word| !word.is_empty())
        .map(|word| format!("\"{word}\"*"))
        .collect();
    (!terms.is_empty()).then(|| terms.join(" "))
}

/// Chinese, Japanese and Korean, written without spaces: the index takes a
/// whole run of them for one word, so only its start would be found.
fn is_cjk(c: char) -> bool {
    matches!(u32::from(c),
        0x3040..=0x30FF | 0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xAC00..=0xD7AF
        | 0xF900..=0xFAFF | 0xFF66..=0xFF9F | 0x20000..=0x2FA1F)
}

/// Some text of `body` around the first `term`, marked as the index's
/// snippets are.
fn snippet_around(body: &str, term: &str) -> String {
    const BEFORE: usize = 20;
    const AFTER: usize = 40;
    let flat = body.replace(['\n', '\r', '\t'], " ");
    let Some(at) = flat.find(term) else {
        return flat.chars().take(BEFORE + AFTER).collect();
    };
    let head = &flat[..at];
    let tail = &flat[at + term.len()..];
    let skipped = head.chars().count().saturating_sub(BEFORE);
    let before: String = head.chars().skip(skipped).collect();
    let after: String = tail.chars().take(AFTER).collect();
    let open = if skipped > 0 { "…" } else { "" };
    let close = if tail.chars().count() > AFTER {
        "…"
    } else {
        ""
    };
    format!("{open}{before}{MATCH_START}{term}{MATCH_END}{after}{close}")
}

impl Store {
    /// Searches titles, bodies and tags of live notes. Title matches rank
    /// highest, then tags, then the body. Matching is case and accent
    /// insensitive.
    ///
    /// # Errors
    ///
    /// Fails only on database errors.
    pub fn search(&self, input: &str, limit: usize) -> Result<Vec<SearchHit>> {
        if input.chars().any(is_cjk) {
            return self.search_text(input, limit);
        }
        let Some(query) = fts_query(input) else {
            return Ok(Vec::new());
        };
        let mut stmt = self.conn.prepare(
            "SELECT n.id, n.title, snippet(notes_fts, 1, ?2, ?3, '…', 12)
             FROM notes_fts JOIN notes n ON n.rowid = notes_fts.rowid
             WHERE notes_fts MATCH ?1 AND n.trashed_at IS NULL
             ORDER BY bm25(notes_fts, 10.0, 1.0, 5.0)
             LIMIT ?4",
        )?;
        let hits = stmt
            .query_map(
                params![
                    query,
                    MATCH_START.to_string(),
                    MATCH_END.to_string(),
                    i64::try_from(limit).unwrap_or(i64::MAX)
                ],
                |row| {
                    Ok(SearchHit {
                        note_id: row.get(0)?,
                        title: row.get(1)?,
                        snippet: row.get(2)?,
                    })
                },
            )?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(hits)
    }

    /// Search for words written in Chinese, Japanese or Korean: every word
    /// anywhere in the title or the text, titles first, then the latest.
    fn search_text(&self, input: &str, limit: usize) -> Result<Vec<SearchHit>> {
        let terms: Vec<&str> = input.split_whitespace().collect();
        let each = "(instr(lower(title), lower(?)) > 0 OR instr(lower(body), lower(?)) > 0)";
        let all = vec![each; terms.len()].join(" AND ");
        let mut stmt = self.conn.prepare(&format!(
            "SELECT id, title, body FROM notes WHERE trashed_at IS NULL AND {all}
             ORDER BY instr(lower(title), lower(?)) = 0, updated_at DESC LIMIT ?"
        ))?;
        let mut values: Vec<rusqlite::types::Value> = terms
            .iter()
            .flat_map(|t| [(*t).to_owned().into(), (*t).to_owned().into()])
            .collect();
        values.push(terms.first().copied().unwrap_or_default().to_owned().into());
        values.push(i64::try_from(limit).unwrap_or(i64::MAX).into());
        let rows = stmt
            .query_map(params_from_iter(values), |row| {
                Ok((
                    row.get(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows
            .into_iter()
            .map(|(note_id, title, body)| {
                let term = terms
                    .iter()
                    .find(|t| body.contains(**t))
                    .copied()
                    .unwrap_or("");
                SearchHit {
                    note_id,
                    snippet: snippet_around(&body, term),
                    title,
                }
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::fts_query;
    use crate::model::{MATCH_END, MATCH_START};
    use crate::store::test_support::store;
    use crate::store::{NewNote, NoteChange};

    fn note(title: &str, body: &str, tags: &[&str]) -> NewNote {
        NewNote {
            title: title.into(),
            body: body.into(),
            tags: tags.iter().map(|t| (*t).to_owned()).collect(),
            ..NewNote::default()
        }
    }

    #[test]
    fn builds_safe_prefix_queries() {
        assert_eq!(fts_query("  tom  ja "), Some("\"tom\"* \"ja\"*".into()));
        assert_eq!(
            fts_query("a\"b OR NOT"),
            Some("\"ab\"* \"OR\"* \"NOT\"*".into())
        );
        assert_eq!(fts_query("   "), None);
    }

    #[test]
    fn finds_chinese_and_japanese_words_inside_a_run() {
        let (mut store, _) = store();
        let ja = store
            .create_note(&note("メモ", "今日は日本語のテキストを書いた", &[]))
            .unwrap();
        let zh = store
            .create_note(&note("中文笔记", "我们今天学习中文", &[]))
            .unwrap();
        store.trash_note(zh.id).unwrap();
        let hits = store.search("テキスト", 10).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].note_id, ja.id);
        assert_eq!(
            hits[0].snippet,
            format!("今日は日本語の{MATCH_START}テキスト{MATCH_END}を書いた")
        );
        assert_eq!(store.search("日本 書いた", 10).unwrap().len(), 1);
        assert!(
            store.search("中文", 10).unwrap().is_empty(),
            "not the trash"
        );
        assert!(store.search("テキスト 无", 10).unwrap().is_empty());
    }

    #[test]
    fn finds_by_title_body_and_tag_with_prefixes_and_accents() {
        let (mut store, _) = store();
        let a = store
            .create_note(&note(
                "Tomato trellis",
                "jute twine between posts",
                &["garden"],
            ))
            .unwrap();
        let b = store
            .create_note(&note(
                "Pain au levain",
                "Levain à 100 %, été comme hiver",
                &["recettes"],
            ))
            .unwrap();

        assert_eq!(store.search("tom", 10).unwrap()[0].note_id, a.id);
        assert_eq!(store.search("TWINE", 10).unwrap()[0].note_id, a.id);
        assert_eq!(store.search("garden", 10).unwrap()[0].note_id, a.id);
        assert_eq!(store.search("ete", 10).unwrap()[0].note_id, b.id);
        assert!(store.search("tomato levain", 10).unwrap().is_empty());
    }

    #[test]
    fn title_matches_rank_above_body_matches() {
        let (mut store, _) = store();
        let in_body = store
            .create_note(&note("Shed", "a ladder is needed", &[]))
            .unwrap();
        let in_title = store
            .create_note(&note("Ladder", "borrow it", &[]))
            .unwrap();
        let hits = store.search("ladder", 10).unwrap();
        assert_eq!(
            hits.iter().map(|h| h.note_id).collect::<Vec<_>>(),
            vec![in_title.id, in_body.id]
        );
    }

    #[test]
    fn snippet_marks_matches_and_index_follows_edits_and_trash() {
        let (mut store, _) = store();
        let n = store
            .create_note(&note("x", "the long ladder on Saturday", &[]))
            .unwrap();
        let hit = &store.search("ladder", 10).unwrap()[0];
        assert!(
            hit.snippet
                .contains(&format!("{MATCH_START}ladder{MATCH_END}"))
        );

        store
            .update_note(
                n.id,
                NoteChange {
                    body: Some("a step stool".into()),
                    ..NoteChange::default()
                },
            )
            .unwrap();
        assert!(store.search("ladder", 10).unwrap().is_empty());
        assert_eq!(store.search("stool", 10).unwrap().len(), 1);

        store.trash_note(n.id).unwrap();
        assert!(store.search("stool", 10).unwrap().is_empty());
        store.restore_note(n.id).unwrap();
        assert_eq!(store.search("stool", 10).unwrap().len(), 1);

        store.set_note_tags(n.id, &["workshop".into()]).unwrap();
        assert_eq!(store.search("workshop", 10).unwrap().len(), 1);
    }

    #[test]
    fn snippet_reads_as_text_not_markdown() {
        let (mut store, _) = store();
        store
            .create_note(&note(
                "x",
                "Ask about the [[Tomato trellis plan]].
> Prune *weekly*",
                &[],
            ))
            .unwrap();
        let hit = &store.search("tomato", 10).unwrap()[0];
        assert!(!hit.snippet.contains("[["), "{}", hit.snippet);
        assert_eq!(store.search("weekly", 10).unwrap().len(), 1);
        assert!(!store.search("weekly", 10).unwrap()[0].snippet.contains('*'));
    }
}
