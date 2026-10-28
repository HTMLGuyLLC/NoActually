//! Popups for moments worth sharing: catch counts, tier ups, streaks.
//! The first sync only records a baseline, so backfilling history never spams notifications.

use crate::{lock, App};
use chrono::Local;
use tauri::AppHandle;
use tauri_plugin_notification::NotificationExt;

const COUNTS: [i64; 9] = [1, 10, 25, 50, 100, 250, 500, 1000, 2500];
const STREAKS: [i64; 5] = [3, 7, 14, 30, 100];
const TIER_ORDER: [&str; 4] = ["Rubber stamp", "Coworker", "Sharp-eyed", "Irreplaceable"];

fn rank(tier: &str) -> usize {
    TIER_ORDER.iter().position(|t| *t == tier).unwrap_or(0)
}

fn ordinal(n: i64) -> String {
    let suffix = match (n % 10, n % 100) {
        (1, x) if x != 11 => "st",
        (2, x) if x != 12 => "nd",
        (3, x) if x != 13 => "rd",
        _ => "th",
    };
    format!("{n}{suffix}")
}

pub fn check(handle: &AppHandle, app: &App) {
    let store = lock(&app.writer);
    let Ok(stats) = store.stats(&Default::default(), Local::now().date_naive()) else { return };
    let get = |k: &str| store.get_setting(k).ok().flatten();
    let first_run = get("milestone.total").is_none();
    let prev_total: i64 = get("milestone.total").and_then(|v| v.parse().ok()).unwrap_or(stats.catches);
    let prev_tier = get("milestone.tier").unwrap_or_else(|| stats.tier.to_string());
    let prev_streak: i64 = get("milestone.streak").and_then(|v| v.parse().ok()).unwrap_or(stats.current_streak_days);
    let _ = store.set_setting("milestone.total", &stats.catches.to_string());
    let _ = store.set_setting("milestone.tier", stats.tier);
    let _ = store.set_setting("milestone.streak", &stats.current_streak_days.to_string());
    drop(store);
    if first_run {
        return;
    }

    let mut notes: Vec<(String, String)> = Vec::new();
    if let Some(m) = COUNTS.iter().rev().find(|m| prev_total < **m && stats.catches >= **m) {
        notes.push((
            format!("Your {} catch!", ordinal(*m)),
            format!("You've caught your AI {m} times. Could it replace you? Not today."),
        ));
    }
    if rank(stats.tier) > rank(&prev_tier) {
        notes.push((
            format!("New tier: {}", stats.tier),
            format!("{} catches in the last 7 days. Open the scorecard to share it.", stats.catches_last_7d),
        ));
    }
    if let Some(s) = STREAKS.iter().rev().find(|s| prev_streak < **s && stats.current_streak_days >= **s) {
        notes.push((format!("{s}-day streak"), format!("You've caught a mistake {s} days in a row.")));
    }
    for (title, body) in notes {
        let _ = handle.notification().builder().title(title).body(body).show();
    }
}
