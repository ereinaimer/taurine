mod setting_delete;
mod setting_get;
mod setting_set;
mod setting_types;

pub use setting_delete::delete_setting;
pub use setting_get::{get_setting, get_setting_value};
pub use setting_set::upsert_setting;
pub use setting_types::SettingRow;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::open_test_db;

    // ── read ──────────────────────────────────────────────────────────────────

    #[test]
    fn get_setting_returns_none_for_missing_key() {
        crate::logs::init_tracing_for_tests();
        let (_dir, conn) = open_test_db();
        let result = get_setting(&conn, "nonexistent").unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn get_setting_value_returns_none_for_missing_key() {
        crate::logs::init_tracing_for_tests();
        let (_dir, conn) = open_test_db();
        let result = get_setting_value(&conn, "nonexistent").unwrap();
        assert!(result.is_none());
    }

    // ── insert (first upsert) ─────────────────────────────────────────────────

    #[test]
    fn upsert_setting_inserts_new_key_with_version_1() {
        crate::logs::init_tracing_for_tests();
        let (_dir, conn) = open_test_db();
        upsert_setting(&conn, "theme", r#""dark""#).unwrap();

        let row = get_setting(&conn, "theme").unwrap().unwrap();
        assert_eq!(row.key, "theme");
        assert_eq!(row.value, r#""dark""#);
        assert_eq!(row.version, 1);
        assert!(row.updated_at > 0);
    }

    #[test]
    fn get_setting_value_returns_value_after_insert() {
        crate::logs::init_tracing_for_tests();
        let (_dir, conn) = open_test_db();
        upsert_setting(&conn, "theme", r#""dark""#).unwrap();

        let value = get_setting_value(&conn, "theme").unwrap().unwrap();
        assert_eq!(value, r#""dark""#);
    }

    // ── update (subsequent upserts) ───────────────────────────────────────────

    #[test]
    fn upsert_setting_increments_version_on_update() {
        crate::logs::init_tracing_for_tests();
        let (_dir, conn) = open_test_db();

        upsert_setting(&conn, "theme", r#""dark""#).unwrap();
        upsert_setting(&conn, "theme", r#""light""#).unwrap();
        upsert_setting(&conn, "theme", r#""system""#).unwrap();

        let row = get_setting(&conn, "theme").unwrap().unwrap();
        assert_eq!(row.version, 3, "version must be 3 after three writes");
        assert_eq!(row.value, r#""system""#);
    }

    #[test]
    fn upsert_setting_updates_value() {
        crate::logs::init_tracing_for_tests();
        let (_dir, conn) = open_test_db();

        upsert_setting(&conn, "lang", r#""en""#).unwrap();
        upsert_setting(&conn, "lang", r#""fr""#).unwrap();

        let value = get_setting_value(&conn, "lang").unwrap().unwrap();
        assert_eq!(value, r#""fr""#);
    }

    #[test]
    fn upsert_setting_does_not_affect_other_keys() {
        crate::logs::init_tracing_for_tests();
        let (_dir, conn) = open_test_db();

        upsert_setting(&conn, "theme", r#""dark""#).unwrap();
        upsert_setting(&conn, "lang", r#""en""#).unwrap();
        upsert_setting(&conn, "theme", r#""light""#).unwrap(); // only theme changes

        let lang_row = get_setting(&conn, "lang").unwrap().unwrap();
        assert_eq!(lang_row.version, 1, "lang must be untouched");
        assert_eq!(lang_row.value, r#""en""#);
    }

    // ── delete ────────────────────────────────────────────────────────────────

    #[test]
    fn delete_setting_returns_true_when_key_exists() {
        crate::logs::init_tracing_for_tests();
        let (_dir, conn) = open_test_db();
        upsert_setting(&conn, "to_delete", r#"true"#).unwrap();

        let deleted = delete_setting(&conn, "to_delete").unwrap();
        assert!(deleted);
    }

    #[test]
    fn delete_setting_returns_false_when_key_missing() {
        crate::logs::init_tracing_for_tests();
        let (_dir, conn) = open_test_db();
        let deleted = delete_setting(&conn, "ghost").unwrap();
        assert!(!deleted);
    }

    #[test]
    fn delete_setting_actually_removes_the_row() {
        crate::logs::init_tracing_for_tests();
        let (_dir, conn) = open_test_db();
        upsert_setting(&conn, "gone", r#"42"#).unwrap();
        delete_setting(&conn, "gone").unwrap();

        let result = get_setting(&conn, "gone").unwrap();
        assert!(result.is_none());
    }
}
