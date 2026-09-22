//! Reactive state for shell IPC.

use std::{collections::HashSet, sync::Arc};

use wayle_core::Property;
use wayle_notification::core::notification::Notification;

/// Shared reactive state exposed to shell components via `ShellIpcService`.
///
/// Bar watchers subscribe to these properties to react to IPC commands.
#[derive(Clone)]
pub struct ShellIpcState {
    /// Connectors whose bars are currently hidden via CLI.
    pub hidden_bars: Property<HashSet<String>>,

    /// All active monitor connectors. Updated by the shell when bars are
    /// created or destroyed.
    pub connectors: Property<Vec<String>>,

    /// Notifications that left the active list (dismissed or expired),
    /// newest first, capped in length. In-memory only, like dunst history.
    pub notification_history: Property<Vec<Arc<Notification>>>,
}

impl ShellIpcState {
    pub(crate) fn new() -> Self {
        Self {
            hidden_bars: Property::new(HashSet::new()),
            connectors: Property::new(Vec::new()),
            notification_history: Property::new(Vec::new()),
        }
    }
}
