//! The activity log as `get_activity` returns it.

use std::collections::VecDeque;

use serde_json::{json, Value};

use crate::session::LogEntry;

/// Entries returned when the caller gives no limit.
pub(crate) const DEFAULT_LIMIT: usize = 50;
/// Most entries one call returns.
pub(crate) const MAX_LIMIT: usize = 200;

/// The entries of `log` after `since` (all of them without `since`), newest last and at most `limit`.
/// `latest_seq` is the last seq the session assigned and `dropped_through` the highest seq that fell out of the log.
pub(crate) fn activity(log: &VecDeque<LogEntry>, latest_seq: u64, dropped_through: u64, since: Option<u64>, limit: Option<usize>) -> Value {
    let after = since.unwrap_or(0);
    let limit = limit.unwrap_or(DEFAULT_LIMIT).min(MAX_LIMIT);
    let newer: Vec<&LogEntry> = log.iter().filter(|e| e.seq > after).collect();
    let cut = newer.len() > limit;
    let kept = &newer[newer.len() - newer.len().min(limit)..];
    let truncated = cut || after < dropped_through || since.is_some_and(|s| s > latest_seq);
    let entries: Vec<Value> = kept
        .iter()
        .map(|e| {
            json!({
                "seq": e.seq,
                "source": e.source,
                "command": e.command,
                "ok": e.ok,
                "mutating": e.mutating,
                "message": e.message,
                "count": e.count,
                "seconds_ago": (e.at.elapsed().as_secs_f64() * 10.0).round() / 10.0,
            })
        })
        .collect();
    json!({"latest_seq": latest_seq, "truncated": truncated, "entries": entries})
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use super::*;
    use crate::session::Source;

    fn log(seqs: impl IntoIterator<Item = u64>) -> VecDeque<LogEntry> {
        seqs.into_iter()
            .map(|seq| LogEntry { seq, source: Source::Gui, command: "set_volume".into(), ok: true, mutating: true, message: String::new(), count: 1, at: Instant::now() })
            .collect()
    }

    fn seqs(v: &Value) -> Vec<u64> {
        v["entries"].as_array().unwrap().iter().map(|e| e["seq"].as_u64().unwrap()).collect()
    }

    #[test]
    fn without_since_returns_the_latest_entries_up_to_the_default_limit() {
        let v = activity(&log(1..=60), 60, 0, None, None);
        let s = seqs(&v);
        assert_eq!(s.len(), 50);
        assert_eq!((s[0], s[49]), (11, 60));
        assert_eq!(v["truncated"], true);
        assert_eq!(v["latest_seq"], 60);
        let v = activity(&log(1..=10), 10, 0, None, None);
        assert_eq!(seqs(&v).len(), 10);
        assert_eq!(v["truncated"], false);
    }

    #[test]
    fn limit_is_capped_and_zero_returns_only_the_counters() {
        let l = log(1..=300);
        assert_eq!(seqs(&activity(&l, 300, 0, None, Some(1000))).len(), 200);
        let z = activity(&l, 300, 0, None, Some(0));
        assert!(seqs(&z).is_empty());
        assert_eq!(z["latest_seq"], 300);
        assert_eq!(z["truncated"], true);
        assert_eq!(activity(&l, 300, 0, Some(300), Some(0))["truncated"], false);
    }

    #[test]
    fn since_keeps_only_newer_entries() {
        let l = log(1..=5);
        let v = activity(&l, 5, 0, Some(3), None);
        assert_eq!(seqs(&v), vec![4, 5]);
        assert_eq!(v["truncated"], false);
        let v = activity(&l, 5, 0, Some(5), None);
        assert!(seqs(&v).is_empty());
        assert_eq!(v["truncated"], false);
    }

    #[test]
    fn truncated_when_since_is_older_than_the_log_or_from_another_run() {
        let l = log(101..=600);
        assert_eq!(activity(&l, 600, 100, Some(50), Some(200))["truncated"], true);
        let v = activity(&l, 600, 100, Some(450), Some(200));
        assert_eq!(seqs(&v), (451..=600).collect::<Vec<_>>());
        assert_eq!(v["truncated"], false);
        // nothing fell out after 100, but 500 newer entries do not fit in 200
        let v = activity(&l, 600, 100, Some(100), Some(200));
        assert_eq!(seqs(&v).len(), 200);
        assert_eq!(v["truncated"], true);
        let v = activity(&l, 600, 100, Some(700), Some(200));
        assert!(seqs(&v).is_empty());
        assert_eq!(v["truncated"], true);
    }

    #[test]
    fn entries_carry_every_field() {
        let mut l = log([7]);
        let e = l.back_mut().unwrap();
        e.source = Source::Mcp;
        e.ok = false;
        e.mutating = false;
        e.count = 3;
        e.message = "x".into();
        let v = activity(&l, 7, 0, None, None);
        let x = &v["entries"][0];
        assert_eq!(x["seq"], 7);
        assert_eq!(x["source"], "mcp");
        assert_eq!(x["command"], "set_volume");
        assert_eq!(x["ok"], false);
        assert_eq!(x["mutating"], false);
        assert_eq!(x["message"], "x");
        assert_eq!(x["count"], 3);
        assert!(x["seconds_ago"].as_f64().unwrap() >= 0.0);
    }
}
