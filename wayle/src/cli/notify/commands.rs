use clap::Subcommand;

/// Notification control subcommands.
#[derive(Subcommand, Debug)]
pub enum NotifyCommands {
    /// List all notifications
    List,

    /// Show recently dismissed/expired notifications
    History {
        /// Number of most recent entries to show
        #[arg(short = 'n', value_name = "COUNT", default_value_t = 10)]
        count: usize,
    },

    /// Dismiss a notification by ID
    Dismiss {
        /// Notification ID to dismiss
        #[arg(value_name = "ID")]
        id: u32,
    },

    /// Dismiss all notifications
    DismissAll,

    /// Toggle Do Not Disturb mode
    Dnd,

    /// Show notification status
    Status,
}
