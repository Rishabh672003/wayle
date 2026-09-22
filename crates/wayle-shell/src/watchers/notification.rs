//! Notification blocklist hot-reload watcher and history collector.

use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use futures::StreamExt;
use serde::Serialize;
use wayle_core::{Property, paths::ConfigPaths};
use wayle_notification::{NotificationService, core::notification::Notification};

use crate::shell::ShellServices;

/// Maximum notifications kept in the in-memory history ring.
const HISTORY_LEN: usize = 50;

/// On-disk history log filename, under `ConfigPaths::state_dir()`.
const HISTORY_FILE: &str = "notification-history.jsonl";

/// Maximum notifications kept in the on-disk history log.
const HISTORY_FILE_MAX: usize = 500;

/// One archived notification, persisted as a JSONL line for `wayle notify history`.
#[derive(Serialize)]
struct HistoryRecord {
    app_name: String,
    summary: String,
    body: String,
    timestamp: i64,
}

/// Syncs the notification blocklist from config to the service on change.
pub fn spawn(services: &ShellServices) {
    let Some(notification) = &services.notification else {
        return;
    };

    let config = services.config.config();
    spawn_blocklist_watcher(&config.modules.notifications, notification);
    spawn_history_collector(notification, services.shell_ipc.state().notification_history);
}

/// Archives notifications that leave the active list (dismissed or expired)
/// into an in-memory history, newest first. Lost on restart, like dunst.
fn spawn_history_collector(
    service: &Arc<NotificationService>,
    history: Property<Vec<Arc<Notification>>>,
) {
    let mut stream = service.notifications.watch();

    tokio::spawn(async move {
        let mut prev: HashMap<u32, Arc<Notification>> = HashMap::new();

        while let Some(current) = stream.next().await {
            let current_ids: HashSet<u32> = current.iter().map(|n| n.id).collect();

            let mut removed: Vec<Arc<Notification>> = prev
                .iter()
                .filter(|(id, _)| !current_ids.contains(id))
                .map(|(_, notif)| notif.clone())
                .collect();
            // Oldest first, so newest ends up at the front after prepending.
            removed.sort_by_key(|n| n.timestamp.get());

            if !removed.is_empty() {
                persist_history(&removed);
                let mut list = history.get();
                for notif in removed {
                    list.insert(0, notif);
                }
                list.truncate(HISTORY_LEN);
                history.set(list);
            }

            prev = current.iter().map(|n| (n.id, n.clone())).collect();
        }
    });
}

/// Persists archived notifications to the on-disk history log so history
/// survives shell restarts. The in-memory ring drives the UI; this file is
/// read by `wayle notify history`. `removed` is oldest-first, keeping the
/// file chronological, capped at the most recent `HISTORY_FILE_MAX`.
// ponytail: rewrites the whole (small, capped) file per event; fine at notification volume.
fn persist_history(removed: &[Arc<Notification>]) {
    let Ok(dir) = ConfigPaths::state_dir() else {
        return;
    };
    let path = dir.join(HISTORY_FILE);

    let mut lines: Vec<String> = std::fs::read_to_string(&path)
        .unwrap_or_default()
        .lines()
        .map(str::to_owned)
        .collect();

    for notif in removed {
        let record = HistoryRecord {
            app_name: notif.app_name.get().unwrap_or_default(),
            summary: notif.summary.get(),
            body: notif.body.get().unwrap_or_default(),
            timestamp: notif.timestamp.get().timestamp(),
        };
        if let Ok(line) = serde_json::to_string(&record) {
            lines.push(line);
        }
    }

    let start = lines.len().saturating_sub(HISTORY_FILE_MAX);
    let _ = std::fs::write(&path, lines[start..].join("\n") + "\n");
}

fn spawn_blocklist_watcher(
    config: &wayle_config::schemas::modules::notification::NotificationConfig,
    service: &Arc<NotificationService>,
) {
    let mut stream = config.blocklist.watch();
    let service = service.clone();

    tokio::spawn(async move {
        stream.next().await;

        while let Some(patterns) = stream.next().await {
            service.set_blocklist(patterns);
        }
    });
}
