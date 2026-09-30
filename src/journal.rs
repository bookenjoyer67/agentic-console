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
