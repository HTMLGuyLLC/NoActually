//! File modes, and the count of report cards left out.

use super::*;
use crate::cards::set_left_out;

#[cfg(unix)]
mod modes {
    use super::*;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;

    fn mode(p: &Path) -> u32 {
        fs::metadata(p).unwrap().permissions().mode() & 0o777
    }

    #[test]
    fn a_new_data_folder_and_database_are_owner_only() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("a").join("b").join(DB_FILE);
        let _store = Store::open(&db).unwrap();
        assert_eq!(mode(db.parent().unwrap()), 0o700);
        assert_eq!(mode(&db), 0o600);
        let wal = dir.path().join("a").join("b").join(format!("{DB_FILE}-wal"));
        assert_eq!(mode(&wal), 0o600, "the log can hold what a database does");
    }

    #[test]
    fn an_existing_install_is_tightened_on_open() {
        let dir = tempfile::tempdir().unwrap();
        let data = dir.path().join(APP_ID);
        fs::create_dir(&data).unwrap();
        let db = data.join(DB_FILE);
        drop(Store::open(&db).unwrap());
        let (wal, shm) = (data.join(format!("{DB_FILE}-wal")), data.join(format!("{DB_FILE}-shm")));
        // Another connection keeps the log files alive while the store opens.
        let keep = Connection::open(&db).unwrap();
        keep.pragma_update(None, "journal_mode", "WAL").unwrap();
        keep.execute_batch("CREATE TABLE IF NOT EXISTS probe (x)").unwrap();
        for p in [&data, &db, &wal, &shm] {
            fs::set_permissions(p, fs::Permissions::from_mode(0o755)).unwrap();
        }
        let _store = Store::open(&db).unwrap();
        assert_eq!(mode(&data), 0o700);
        for p in [&db, &wal, &shm] {
            assert_eq!(mode(p), 0o600, "{}", p.display());
        }
    }

    #[test]
    fn a_folder_chosen_by_the_user_is_left_alone_and_markers_stay_readable() {
        let dir = tempfile::tempdir().unwrap();
        fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o755)).unwrap();
        let db = dir.path().join(DB_FILE);
        let store = Store::open(&db).unwrap();
        assert_eq!(mode(dir.path()), 0o755, "it may hold other things");
        assert_eq!(mode(&db), 0o600);
        store.revoke("codex").unwrap();
        assert_eq!(mode(&dir.path().join("source-stopped-codex")), 0o644, "the hooks read it");
    }
}

#[test]
fn left_out_cards_are_counted_per_file_and_not_twice() {
    let mut s = Store::open_in_memory().unwrap();
    let set = |path: &str, n| set_left_out(s.conn(), "codex", path, n).unwrap();
    set("/a", 2);
    set("/a", 2); // the same file read again: still 2
    set("/b", 1);
    assert_eq!(s.report_cards_left_out("codex").unwrap(), 3);
    let set = |path: &str, n| set_left_out(s.conn(), "codex", path, n).unwrap();
    set("/a", 0); // its cards save fine now
    assert_eq!(s.report_cards_left_out("codex").unwrap(), 1);
    assert_eq!(s.report_cards_left_out("codex-other").unwrap(), 0, "another source's prefix is not this one");
    s.reset_scores().unwrap();
    assert_eq!(s.report_cards_left_out("codex").unwrap(), 0, "a rebuild counts them again from zero");
}

#[test]
fn forgetting_a_source_clears_only_its_left_out_count() {
    let mut s = Store::open_in_memory().unwrap();
    set_left_out(s.conn(), "codex", "/a", 2).unwrap();
    set_left_out(s.conn(), "codex-two", "/b", 5).unwrap();
    s.forget("codex").unwrap();
    assert_eq!(s.report_cards_left_out("codex").unwrap(), 0);
    assert_eq!(s.report_cards_left_out("codex-two").unwrap(), 5);
}
