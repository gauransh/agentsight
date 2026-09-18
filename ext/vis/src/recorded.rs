//! A bounded import of recorded operation evidence into the existing renderer.
//! This never discovers sessions, executes commands, or reads referenced paths.
use crate::RepositoryTrace;
use serde::Deserialize;
use std::collections::HashSet;
use std::fs::File;
use std::io::{self, Read};
use std::path::Path;

const MAX_BYTES: u64 = 16 * 1024 * 1024;
const MAX_EVENTS: usize = 10_000;
const MAX_ACTIONS: usize = 100_000;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RecordedTrace {
    run_id: String,
    pub partial: bool,
    pub trace: RepositoryTrace,
}

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

fn bounded(value: &str, limit: usize) -> bool {
    value.len() <= limit && !value.chars().any(char::is_control)
}

pub(crate) fn read_trace(path: &Path, run_id: Option<&str>) -> io::Result<RecordedTrace> {
    let mut bytes = Vec::new();
    File::open(path)?
        .take(MAX_BYTES + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err(invalid("recorded trace exceeds the 16 MiB input bound"));
    }
    let mut input: RecordedTrace = serde_json::from_slice(&bytes)
        .map_err(|_| invalid("recorded trace is not a valid trace envelope"))?;
    let expected = run_id.ok_or_else(|| invalid("--trace-json requires --run-id"))?;
    if expected.is_empty() || !bounded(expected, 256) || input.run_id != expected {
        return Err(invalid("recorded trace run_id does not match --run-id"));
    }
    let trace = &mut input.trace;
    if !bounded(&trace.repository, 4096)
        || !bounded(&trace.revision, 256)
        || trace.events.len() > MAX_EVENTS
        || trace.global
        || trace.session_count > 1
        || trace.start_ms < 0
        || trace.end_ms < trace.start_ms
        || !trace.commits_ms.is_empty()
    {
        return Err(invalid(
            "recorded trace contains invalid scope, bounds, or commit claims",
        ));
    }
    let mut ids = HashSet::new();
    let mut actions = 0_usize;
    for event in &trace.events {
        if event.id.is_empty()
            || !bounded(&event.id, 512)
            || !ids.insert(&event.id)
            || event.session_id != expected
            || event.vendor != "recorded-activity"
            || !matches!(
                event.tool_name.as_str(),
                "FileOpen" | "FileOpenReadOnly" | "FileOpenReadWrite"
            )
            || event.category != "file"
            || event.command_name != event.tool_name
            || !matches!(event.status.as_str(), "allowed" | "denied" | "unknown")
            || event.ts_ms < trace.start_ms
            || event.ts_ms > trace.end_ms
        {
            return Err(invalid(
                "recorded event has invalid identity, time, or field bounds",
            ));
        }
        actions = actions.saturating_add(event.actions.len());
        if actions > MAX_ACTIONS {
            return Err(invalid("recorded trace exceeds the file action bound"));
        }
        for action in &event.actions {
            if action.path.is_empty()
                || !bounded(&action.path, 4096)
                || action.access != "open"
                || action.previous_path.is_some()
            {
                return Err(invalid("recorded file action has invalid path or access"));
            }
        }
    }
    if trace.file_action_count != actions || trace.source_event_count < trace.events.len() {
        return Err(invalid("recorded trace counts do not match its evidence"));
    }
    trace
        .events
        .sort_by(|a, b| (a.ts_ms, &a.id).cmp(&(b.ts_ms, &b.id)));
    Ok(input)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};

    #[test]
    fn rendered_import_declares_its_source_without_claiming_a_native_session() {
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("trace.json");
        let output = dir.path().join("graph.html");
        std::fs::write(&input, serde_json::to_vec(&document()).unwrap()).unwrap();
        crate::run_vis_with_trace(
            dir.path(),
            std::slice::from_ref(&output),
            false,
            crate::CompactRate::Full,
            None,
            Some("run-1"),
            Some(&input),
        )
        .unwrap();
        let html = std::fs::read_to_string(output).unwrap();
        let (_, body) = html.rsplit_once("AgentVis.initialize(").unwrap();
        let mut stream = serde_json::Deserializer::from_str(body).into_iter::<Value>();
        let payload = stream.next().unwrap().unwrap();
        assert_eq!(payload["meta"]["activity_source"], "recorded_file_activity");
        assert_eq!(payload["meta"]["activity_partial"], true);
        assert_eq!(
            payload["meta"]["session_scope"],
            "operation_recorded_activity"
        );
        assert!(payload["meta"]["session_id"].is_null());
        assert_eq!(payload["events"][0]["status"], "denied");
        assert_eq!(payload["events"][0]["actions"][0]["access"], "open");
        assert!(payload["commits"].as_array().unwrap().is_empty());
    }

    fn document() -> Value {
        json!({"run_id":"run-1","partial":true,"trace":{
            "repository":"operation","revision":"","start_ms":1000,"end_ms":2000,
            "global":false,"session_count":1,"source_event_count":1,"file_action_count":1,
            "commits_ms":[],"events":[{"id":"observed-1","session_id":"run-1",
                "vendor":"recorded-activity","ts_ms":1500,"tool_name":"FileOpen","category":"file",
                "command_name":"FileOpen","status":"denied",
                "actions":[{"path":"/tmp/file","access":"open"}]}]}})
    }

    fn read(value: &Value, id: Option<&str>) -> io::Result<RecordedTrace> {
        let file = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(file.path(), serde_json::to_vec(value).unwrap()).unwrap();
        read_trace(file.path(), id)
    }

    #[test]
    fn recorded_open_retains_denial_and_operation_group_without_path_reads() {
        let parsed = read(&document(), Some("run-1")).unwrap();
        assert!(parsed.partial);
        assert_eq!(parsed.trace.events[0].status, "denied");
        assert_eq!(parsed.trace.events[0].actions[0].access, "open");
        assert_eq!(parsed.trace.events[0].session_id, "run-1");
    }

    #[test]
    fn rejects_wrong_run_mixed_session_duplicate_events_and_unearned_commits() {
        assert!(read(&document(), Some("another-run")).is_err());
        assert!(read(&document(), None).is_err());
        let mut value = document();
        value["trace"]["events"][0]["session_id"] = json!("another-run");
        assert!(read(&value, Some("run-1")).is_err());
        let mut value = document();
        let event = value["trace"]["events"][0].clone();
        value["trace"]["events"].as_array_mut().unwrap().push(event);
        assert!(read(&value, Some("run-1")).is_err());
        let mut value = document();
        value["trace"]["commits_ms"] = json!([1500]);
        assert!(read(&value, Some("run-1")).is_err());
    }

    #[test]
    fn rejects_unbounded_paths_times_counts_and_input() {
        for (field, replacement) in [
            ("path", json!("x".repeat(4097))),
            ("access", json!("execute")),
        ] {
            let mut value = document();
            value["trace"]["events"][0]["actions"][0][field] = replacement;
            assert!(read(&value, Some("run-1")).is_err());
        }
        let mut value = document();
        value["trace"]["events"][0]["ts_ms"] = json!(2001);
        assert!(read(&value, Some("run-1")).is_err());
        let mut value = document();
        value["trace"]["file_action_count"] = json!(2);
        assert!(read(&value, Some("run-1")).is_err());
        let file = tempfile::NamedTempFile::new().unwrap();
        file.as_file().set_len(MAX_BYTES + 1).unwrap();
        assert!(read_trace(file.path(), Some("run-1")).is_err());
    }

    #[test]
    fn empty_recorded_operation_is_valid_without_inventing_activity() {
        let mut value = document();
        value["trace"]["events"] = json!([]);
        value["trace"]["source_event_count"] = json!(0);
        value["trace"]["file_action_count"] = json!(0);
        value["trace"]["session_count"] = json!(0);
        assert!(read(&value, Some("run-1")).unwrap().trace.events.is_empty());
    }
}
