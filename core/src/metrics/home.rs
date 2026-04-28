use rusqlite::{Connection, types::Type};

use crate::db::crud::{TriggerType, get_current_os_db_string};

const DEFAULT_MOST_USED_LIMIT: usize = 5;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct HomeMetrics {
    pub keystrokes_saved: u64,
    pub time_saved_ms: u64,
    pub expansions_run: u64,
    pub most_used: Vec<MostUsedAutomation>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MostUsedAutomation {
    pub trigger: String,
    pub trigger_type: TriggerType,
    pub uses: u64,
    pub preview: String,
}

pub fn load_home_metrics(conn: &Connection) -> crate::Result<HomeMetrics> {
    load_home_metrics_with_limit(conn, DEFAULT_MOST_USED_LIMIT)
}

pub fn load_home_metrics_with_limit(conn: &Connection, limit: usize) -> crate::Result<HomeMetrics> {
    let (expansions_run, keystrokes_saved, time_saved_ms) = conn.query_row(
        "SELECT
            COALESCE(SUM(executions), 0),
            COALESCE(SUM(keystrokes_saved), 0),
            COALESCE(SUM(time_saved_ms), 0)
         FROM metrics",
        [],
        |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, i64>(2)?,
            ))
        },
    )?;

    let os_str = get_current_os_db_string();
    let mut stmt = conn.prepare_cached(
        "SELECT
            a.trigger,
            a.trigger_type,
            a.usage_count,
            a.output,
            a.action_type,
            a.name,
            a.description
         FROM automations a
         WHERE a.is_deleted = 0
           AND a.is_enabled = 1
           AND a.usage_count > 0
           AND (a.target_os = 'all' OR a.target_os = ?1)
         ORDER BY (a.target_os != 'all') DESC,
                  a.usage_count DESC,
                  a.updated_at DESC,
                  LOWER(a.trigger) ASC,
                  a.trigger ASC
         LIMIT ?2",
    )?;

    let rows = stmt.query_map((os_str, limit as i64), |row| {
        let trigger_type = TriggerType::parse_db(&row.get::<_, String>(1)?).map_err(|err| {
            rusqlite::Error::FromSqlConversionFailure(1, Type::Text, Box::new(err))
        })?;

        Ok(MostUsedAutomation {
            trigger: row.get(0)?,
            trigger_type,
            uses: row.get::<_, i64>(2)?.max(0) as u64,
            preview: preview_for_row(
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, String>(5)?,
                row.get::<_, Option<String>>(6)?,
            ),
        })
    })?;

    let mut most_used = Vec::new();
    for row in rows {
        most_used.push(row?);
    }

    Ok(HomeMetrics {
        keystrokes_saved: keystrokes_saved.max(0) as u64,
        time_saved_ms: time_saved_ms.max(0) as u64,
        expansions_run: expansions_run.max(0) as u64,
        most_used,
    })
}

fn preview_for_row(
    output: String,
    action_type: String,
    name: String,
    description: Option<String>,
) -> String {
    let description = description.unwrap_or_default();
    let preview = match action_type.as_str() {
        "script" => first_non_empty([description.as_str(), output.as_str(), name.as_str()]),
        _ => first_non_empty([output.as_str(), description.as_str(), name.as_str()]),
    };

    preview.to_string()
}

fn first_non_empty<'a>(values: impl IntoIterator<Item = &'a str>) -> &'a str {
    values
        .into_iter()
        .map(str::trim)
        .find(|value| !value.is_empty())
        .unwrap_or("")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::crud::{
        TriggerType, delete_automation, increment_metric, upsert_automation,
        upsert_automation_with_trigger_type,
    };
    use crate::testing::{init_tracing_for_tests, open_test_db};

    #[test]
    fn empty_home_metrics_returns_zero_totals_and_no_automations() {
        init_tracing_for_tests();
        let (_dir, conn) = open_test_db();

        let metrics = load_home_metrics(&conn).unwrap();

        assert_eq!(metrics.keystrokes_saved, 0);
        assert_eq!(metrics.time_saved_ms, 0);
        assert_eq!(metrics.expansions_run, 0);
        assert!(metrics.most_used.is_empty());
    }

    #[test]
    fn home_metrics_aggregate_totals_and_sort_automations() {
        init_tracing_for_tests();
        let (_dir, conn) = open_test_db();

        increment_metric(&conn, "2026-04-01", 4, 0, 120, 180_000).unwrap();
        increment_metric(&conn, "2026-04-02", 2, 1, 30, 60_000).unwrap();

        upsert_automation(
            &conn,
            "uuid-word",
            "Git Status",
            Some("git status"),
            "gs",
            "git status",
            "text",
            "all",
            "[]",
            12,
            None,
        )
        .unwrap();
        upsert_automation_with_trigger_type(
            &conn,
            "uuid-hotkey",
            "Email Signature",
            Some("personal email signature"),
            TriggerType::Hotkey,
            "ralt+m",
            "[Script: powershell]",
            "script",
            "all",
            "[]",
            20,
            None,
        )
        .unwrap();
        upsert_automation(
            &conn,
            "uuid-deleted",
            "Old Trigger",
            None,
            "old",
            "old output",
            "text",
            "all",
            "[]",
            99,
            None,
        )
        .unwrap();
        delete_automation(&conn, "uuid-deleted").unwrap();
        upsert_automation(
            &conn,
            "uuid-disabled",
            "Disabled Trigger",
            None,
            "disabled",
            "should not render",
            "text",
            "all",
            "[]",
            77,
            None,
        )
        .unwrap();
        conn.execute(
            "UPDATE automations SET is_enabled = 0 WHERE id = 'uuid-disabled'",
            [],
        )
        .unwrap();

        let metrics = load_home_metrics(&conn).unwrap();

        assert_eq!(metrics.expansions_run, 6);
        assert_eq!(metrics.keystrokes_saved, 150);
        assert_eq!(metrics.time_saved_ms, 240_000);
        assert_eq!(metrics.most_used.len(), 2);
        assert_eq!(metrics.most_used[0].trigger, "ralt+m");
        assert_eq!(metrics.most_used[0].trigger_type, TriggerType::Hotkey);
        assert_eq!(metrics.most_used[0].uses, 20);
        assert_eq!(metrics.most_used[0].preview, "personal email signature");
        assert_eq!(metrics.most_used[1].trigger, "gs");
    }

    #[test]
    fn most_used_limit_is_applied() {
        init_tracing_for_tests();
        let (_dir, conn) = open_test_db();

        for index in 0..6 {
            let id = format!("uuid-{index}");
            let trigger = format!("t{index}");
            upsert_automation(
                &conn,
                &id,
                &format!("Automation {index}"),
                None,
                &trigger,
                &format!("Output {index}"),
                "text",
                "all",
                "[]",
                (10 - index) as i64,
                None,
            )
            .unwrap();
        }

        let metrics = load_home_metrics_with_limit(&conn, 5).unwrap();

        assert_eq!(metrics.most_used.len(), 5);
        assert_eq!(metrics.most_used[0].uses, 10);
        assert_eq!(metrics.most_used[4].uses, 6);
    }
}
