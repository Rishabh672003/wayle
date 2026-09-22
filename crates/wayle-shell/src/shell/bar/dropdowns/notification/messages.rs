use std::sync::Arc;

use wayle_config::ConfigService;
use wayle_core::Property;
use wayle_notification::{NotificationService, core::notification::Notification};

pub(crate) struct NotificationDropdownInit {
    pub notification: Arc<NotificationService>,
    pub config: Arc<ConfigService>,
    /// Recently dismissed/expired notifications, newest first (shell IPC state).
    pub history: Property<Vec<Arc<Notification>>>,
}

#[derive(Debug)]
pub(crate) enum NotificationDropdownMsg {
    DndToggled(bool),
    ClearAll,
    NotificationDismissed,
    ToggleHistory,
    HistoryItemDismissed(u32),
}

#[derive(Debug)]
pub(crate) enum NotificationDropdownCmd {
    NotificationsChanged,
    DndChanged(bool),
    ScaleChanged(f32),
    IconSourceChanged,
    TimeTick,
    HistoryChanged,
}
