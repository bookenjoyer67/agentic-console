//! Parsers for the three append-only journals.
//!
//! One JSON object per line, written by the servers themselves. The console reads them as records and
//! shows the last N; it never writes to one. An unparseable line is skipped rather than guessed at.

use std::time::SystemTime;

use serde_json::Value;

use crate::iso;

/// One line of the gate journal: one executed gate.
#[derive(Clone, Debug, PartialEq)]
pub struct GateEntry {
    pub timestamp: String,
    pub at: Option<SystemTime>,
    pub gate: String,
    pub role: String,
    pub exit_code: i64,
    pub duration: f64,
    pub passed: bool,
    pub guard_applied: bool,
    pub guard_satisfied: bool,
    pub argv: Vec<String>,
}

impl GateEntry {
    /// The guard column: `not applied`, `applied ok`, `applied UNSATISFIED`.
    pub fn guard_text(&self) -> String {
        if !self.guard_applied {
            "not applied".to_string()
        } else if self.guard_satisfied {
            "applied ok".to_string()
        } else {
            "applied UNSATISFIED".to_string()
        }
    }

    /// `argv` as one space-joined line, for the provenance pane.
    pub fn argv_text(&self) -> String {
        self.argv.join(" ")
    }
}

/// One line of the storage journal: one authorised or refused storage call.
///
/// The journal is the audit log and records the *event* -- which role called which operation against
/// which entry id -- never the stored record's title or body; `mcp/storage/SCHEMA.md` lists the
/// audit record's keys and no title is among them. `title` is therefore filled in two ways: from the
/// journal line itself when a line carries one, and otherwise from the storage database beside the
/// journal, whose `entries` table the server's own `list_entries` reads metadata (including the
/// title) out of. It is what lets the console tell a close from the verdict before it: see
/// `checkpoint::states_a_close`.
#[derive(Clone, Debug, PartialEq)]
pub struct StoreEntry {
    pub timestamp: String,
    pub at: Option<SystemTime>,
    pub role: String,
    pub operation: String,
    pub entry_id: String,
    pub classification: String,
    pub allowed: bool,
    pub reason: String,
    /// The stored record's own title, or empty when the console could not read one.
    pub title: String,
}

impl StoreEntry {
    /// The first eight characters of the entry id, enough to match a role's return value by eye.
    pub fn short_id(&self) -> String {
        self.entry_id.chars().take(8).collect()
    }
}

/// One line of the retrieval journal: one authorised or refused retrieval.
#[derive(Clone, Debug, PartialEq)]
pub struct RetrieveEntry {
    pub timestamp: String,
    pub at: Option<SystemTime>,
    pub role: String,
    pub ceiling: String,
    pub decision: String,
    pub result_count: i64,
}

fn field(record: &Value, key: &str) -> String {
    record
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

fn int(record: &Value, key: &str) -> i64 {
    record.get(key).and_then(Value::as_i64).unwrap_or_default()
}

fn flag(record: &Value, key: &str) -> bool {
    record.get(key).and_then(Value::as_bool).unwrap_or(false)
}

/// Split a journal into parsed JSON objects, skipping blank and unparseable lines.
pub fn json_lines(text: &str) -> Vec<Value> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter(Value::is_object)
        .collect()
}

/// Parse the gate journal, newest last, keeping at most `limit` records.
pub fn parse_gate_journal(text: &str, limit: usize) -> Vec<GateEntry> {
    let mut entries: Vec<GateEntry> = json_lines(text)
        .iter()
        .map(|record| GateEntry {
            timestamp: field(record, "timestamp"),
            at: iso::parse_iso8601(&field(record, "timestamp")),
            gate: field(record, "gate"),
            role: field(record, "calling_role"),
            exit_code: int(record, "exit_code"),
            duration: record
                .get("duration_seconds")
                .and_then(Value::as_f64)
                .unwrap_or_default(),
            passed: flag(record, "passed"),
            guard_applied: flag(record, "guard_applied"),
            guard_satisfied: flag(record, "guard_satisfied"),
            argv: record
                .get("argv")
                .and_then(Value::as_array)
                .map(|list| {
                    list.iter()
                        .filter_map(Value::as_str)
                        .map(str::to_string)
                        .collect::<Vec<String>>()
                })
                .unwrap_or_default(),
        })
        .collect();
    if entries.len() > limit {
        entries.drain(..entries.len() - limit);
    }
    entries
}

/// Parse the storage journal, newest last, keeping at most `limit` records.
pub fn parse_storage_journal(text: &str, limit: usize) -> Vec<StoreEntry> {
    let mut entries: Vec<StoreEntry> = json_lines(text)
        .iter()
        .map(|record| StoreEntry {
            timestamp: field(record, "timestamp"),
            at: iso::parse_iso8601(&field(record, "timestamp")),
            role: field(record, "calling_role"),
            operation: field(record, "operation"),
            entry_id: field(record, "entry_id"),
            classification: field(record, "classification"),
            allowed: record
                .get("allowed")
                .and_then(Value::as_bool)
                .unwrap_or(true),
            reason: field(record, "reason"),
            title: field(record, "title"),
        })
        .collect();
    if entries.len() > limit {
        entries.drain(..entries.len() - limit);
    }
    entries
}

/// Parse the retrieval journal, newest last, keeping at most `limit` records.
pub fn parse_retrieval_journal(text: &str, limit: usize) -> Vec<RetrieveEntry> {
    let mut entries: Vec<RetrieveEntry> = json_lines(text)
        .iter()
        .map(|record| RetrieveEntry {
            timestamp: field(record, "timestamp"),
            at: iso::parse_iso8601(&field(record, "timestamp")),
            role: field(record, "calling_role"),
            ceiling: field(record, "effective_ceiling"),
            decision: field(record, "decision"),
            result_count: int(record, "result_count"),
        })
        .collect();
    if entries.len() > limit {
        entries.drain(..entries.len() - limit);
    }
    entries
}

/// The last journal record written by `role`, or `None` when the journal holds none for it.
pub fn last_by_role<'a, T, F>(entries: &'a [T], role: F, wanted: &str) -> Option<&'a T>
where
    F: Fn(&'a T) -> &'a str,
{
    entries.iter().rev().find(|entry| role(entry) == wanted)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_storage_record_carries_its_own_title_when_the_line_names_one() {
        // The audit record the storage server writes has no title (see `StoreEntry`), so this is the
        // read that catches a line that *does* name one -- and an id with none stays empty rather
        // than borrowing another record's title.
        let line = "{\"allowed\": true, \"calling_role\": \"reviewer\", \
                    \"classification\": \"internal\", \"entry_id\": \"3794f97e-976e-4d0b-8507-de28a56f2ab0\", \
                    \"operation\": \"write_entry\", \"project_id\": \"proj-komun\", \"reason\": null, \
                    \"timestamp\": \"2026-09-30T17:28:16.869579+00:00\", \
                    \"title\": \"Closing record — act2-v2-run1 (KOMUN-act2-v2-run1) is Done: delivered scope\"}\n";
        let entries = parse_storage_journal(line, 12);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].role, "reviewer");
        assert_eq!(
            entries[0].title,
            "Closing record — act2-v2-run1 (KOMUN-act2-v2-run1) is Done: delivered scope"
        );

        let untitled = parse_storage_journal(
            "{\"allowed\": true, \"calling_role\": \"planner\", \"entry_id\": \"e\", \
             \"operation\": \"write_entry\", \"timestamp\": \"2026-09-30T15:11:10.247308+00:00\"}\n",
            12,
        );
        assert_eq!(untitled.len(), 1);
        assert!(untitled[0].title.is_empty());
    }
}
