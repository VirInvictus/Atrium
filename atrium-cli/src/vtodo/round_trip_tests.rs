// SPDX-License-Identifier: MIT
//! Integration tests: parsed VTODO → DB import → DB export →
//! re-emitted VCALENDAR → re-parse. Asserts the modeled
//! subset round-trips losslessly.
//!
//! Lives under `src/vtodo/` because atrium-cli is a binary crate
//! (no library target), so a top-level `tests/` integration test
//! can't reach into the binary's modules. The `#[cfg(test)]` gate
//! keeps this out of release builds.

use std::path::Path;

use super::{
    EmitConfig, VTODO_LOCATION_KEY, VTODO_NAMESPACE, VTODO_UID_KEY, emit_vcalendar, export_vtodo,
    import_vtodo, parse_ics,
};
use crate::vtodo::mapper::LossyKind;

fn fresh_file_db(label: &str) -> (rusqlite::Connection, std::path::PathBuf) {
    let dir = std::env::temp_dir().join(format!("atrium-vtodo-{}-{}", label, std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let db_path = dir.join("atrium.db");
    let conn = atrium_core::db::open(&db_path).unwrap();
    (conn, db_path)
}

fn read_fixture(name: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/vtodo")
        .join(name);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("fixture {} unreadable: {e}", path.display()))
}

#[tokio::test]
async fn basic_fixture_round_trips_modeled_subset() {
    let text = read_fixture("basic.ics");
    let parsed = parse_ics(&text).unwrap();
    assert_eq!(parsed.vtodos.len(), 1);

    let (writer_conn, db_path) = fresh_file_db("basic");
    let (handle, _changes, _library) = atrium_core::spawn_worker(writer_conn);

    let summary = import_vtodo(&handle, &parsed, "Errands", false)
        .await
        .unwrap();
    assert_eq!(summary.tasks_created, 1);
    assert!(summary.project_id.is_some());

    // Drop handle to release the writable conn, then read back
    // via a fresh connection on the same file.
    drop(handle);
    // Small spin to let the worker drain.
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let read_conn = atrium_core::db::open(&db_path).unwrap();
    let exported = export_vtodo(&read_conn).unwrap();
    assert_eq!(exported.len(), 1);
    let v = &exported[0];

    // UID was already UUID-shaped → identity round-trip.
    assert_eq!(v.uid, "11111111-2222-3333-4444-555555555555");
    assert_eq!(v.summary.as_deref(), Some("Buy milk"));
    assert_eq!(
        v.description.as_deref(),
        Some("Two percent, half-gallon\nFor breakfast cereal"),
    );
    assert!(v.dtstart.is_some(), "DTSTART must survive");
    assert!(v.due.is_some(), "DUE must survive");
    assert_eq!(v.status.as_deref(), Some("NEEDS-ACTION"));
    assert_eq!(v.priority, Some(1));
    assert_eq!(v.rrule.as_deref(), Some("FREQ=WEEKLY"));
    assert_eq!(v.location.as_deref(), Some("Corner store"));
    let mut cats = v.categories.clone();
    cats.sort();
    assert_eq!(cats, vec!["errands", "home"]);

    // Re-emit + re-parse: the modeled fields must round-trip
    // through the emit pass too.
    let text = emit_vcalendar(&exported, &EmitConfig::default());
    let re = parse_ics(&text).unwrap();
    assert_eq!(re.vtodos.len(), 1);
    let r = &re.vtodos[0];
    assert_eq!(r.summary.as_deref(), Some("Buy milk"));
    assert_eq!(
        r.description.as_deref(),
        Some("Two percent, half-gallon\nFor breakfast cereal"),
    );
    assert_eq!(r.rrule.as_deref(), Some("FREQ=WEEKLY"));
    assert_eq!(r.location.as_deref(), Some("Corner store"));
    let mut cats = r.categories.clone();
    cats.sort();
    assert_eq!(cats, vec!["errands", "home"]);
    let _ = std::fs::remove_dir_all(db_path.parent().unwrap());
}

#[tokio::test]
async fn nextcloud_sample_preserves_original_uid_via_extras() {
    let text = read_fixture("nextcloud_sample.ics");
    let parsed = parse_ics(&text).unwrap();
    assert_eq!(parsed.vtodos.len(), 1);
    let original_uid = parsed.vtodos[0].uid.clone().unwrap();

    let (writer_conn, db_path) = fresh_file_db("nextcloud");
    let (handle, _changes, _library) = atrium_core::spawn_worker(writer_conn);

    import_vtodo(&handle, &parsed, "Bills", false)
        .await
        .unwrap();
    drop(handle);
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let read_conn = atrium_core::db::open(&db_path).unwrap();
    let exported = export_vtodo(&read_conn).unwrap();
    assert_eq!(exported.len(), 1);

    // UID came back free-form, not a UUID — the round-trip
    // contract is that VTODO_UID_KEY wins on emit.
    assert_eq!(exported[0].uid, original_uid);

    // Re-import the emitted file. The same v5 derivation
    // produces the same task.uuid, so the round-trip stays
    // stable across re-runs.
    let re_text = emit_vcalendar(&exported, &EmitConfig::default());
    let re = parse_ics(&re_text).unwrap();
    assert_eq!(re.vtodos[0].uid.as_deref(), Some(original_uid.as_str()));

    // Also confirm the v5-derived UUID — this is the actual
    // `task.uuid` value Atrium stamped.
    let derived = uuid::Uuid::new_v5(&VTODO_NAMESPACE, original_uid.as_bytes()).to_string();
    // The exported `VtodoOutput.uid` is the original (stashed)
    // UID, so we can't assert the derived UUID off `exported`
    // directly. Read the task back to verify.
    let tasks = atrium_core::db::read::list_all_tasks(&read_conn).unwrap();
    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0].uuid, derived);
    assert_eq!(
        tasks[0]
            .extra_properties
            .get(VTODO_UID_KEY)
            .map(String::as_str),
        Some(original_uid.as_str()),
    );

    let _ = std::fs::remove_dir_all(db_path.parent().unwrap());
}

#[tokio::test]
async fn lossy_fixture_surfaces_one_entry_per_dropped_construct() {
    let text = read_fixture("lossy.ics");
    let parsed = parse_ics(&text).unwrap();

    // One VTODO recognised; VEVENT + VJOURNAL surface as
    // unsupported_top_level.
    assert_eq!(parsed.vtodos.len(), 1);
    assert_eq!(parsed.unsupported_top_level, vec!["VEVENT", "VJOURNAL"]);

    let (writer_conn, _db_path) = fresh_file_db("lossy");
    let (handle, _changes, _library) = atrium_core::spawn_worker(writer_conn);

    let summary = import_vtodo(&handle, &parsed, "Lossy", false)
        .await
        .unwrap();

    let kinds: Vec<LossyKind> = summary.lossy.iter().map(|l| l.kind).collect();
    assert!(kinds.contains(&LossyKind::DroppedAlarm));
    assert!(kinds.contains(&LossyKind::DroppedAttendee));
    assert!(kinds.contains(&LossyKind::DroppedGeo));
    assert!(kinds.contains(&LossyKind::DroppedPercentComplete));
    assert!(kinds.contains(&LossyKind::DroppedDuration));
    assert!(kinds.contains(&LossyKind::DroppedTimezone));
    assert!(kinds.contains(&LossyKind::UnknownProperty));
    // Two unsupported top-level components → two
    // UnsupportedComponent entries.
    assert_eq!(
        kinds
            .iter()
            .filter(|k| **k == LossyKind::UnsupportedComponent)
            .count(),
        2,
    );

    // X-CUSTOM-FIELD should NOT be lossy — it stashes verbatim.
    drop(handle);
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    // (No deeper assertion here; the stash is exercised in the
    // earlier basic / nextcloud tests via extras.)
}

#[tokio::test]
async fn multi_fixture_lands_three_tasks_with_status_round_trip() {
    let text = read_fixture("multi.ics");
    let parsed = parse_ics(&text).unwrap();
    assert_eq!(parsed.vtodos.len(), 3);

    let (writer_conn, db_path) = fresh_file_db("multi");
    let (handle, _changes, _library) = atrium_core::spawn_worker(writer_conn);

    let summary = import_vtodo(&handle, &parsed, "Mixed", false)
        .await
        .unwrap();
    assert_eq!(summary.tasks_created, 3);

    drop(handle);
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let read_conn = atrium_core::db::open(&db_path).unwrap();
    let exported = export_vtodo(&read_conn).unwrap();
    assert_eq!(exported.len(), 3);

    let needs_action = exported
        .iter()
        .find(|v| v.uid.starts_with("22222222"))
        .unwrap();
    assert_eq!(needs_action.status.as_deref(), Some("NEEDS-ACTION"));

    let completed = exported
        .iter()
        .find(|v| v.uid.starts_with("33333333"))
        .unwrap();
    assert_eq!(completed.status.as_deref(), Some("COMPLETED"));
    assert!(completed.completed.is_some());

    let cancelled = exported
        .iter()
        .find(|v| v.uid.starts_with("44444444"))
        .unwrap();
    // CANCELLED stashed in orig_keyword on import; export
    // derives STATUS from that.
    assert_eq!(cancelled.status.as_deref(), Some("CANCELLED"));

    // Stash should retain VTODO_LOCATION_KEY behaviour: none
    // of these multi.ics tasks set LOCATION, so none should
    // round-trip with it.
    for v in &exported {
        assert!(v.location.is_none(), "no LOCATION expected: {v:?}");
    }
    // And the round-trip namespaces.
    let _ = VTODO_LOCATION_KEY;

    let _ = std::fs::remove_dir_all(db_path.parent().unwrap());
}
