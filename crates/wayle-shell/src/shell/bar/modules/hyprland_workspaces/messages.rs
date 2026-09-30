use std::{collections::HashMap, sync::Arc};

use wayle_config::ConfigService;
use wayle_hyprland::{Address, HyprlandService};
use wayle_widgets::prelude::BarSettings;

pub(crate) struct WorkspacesInit {
    pub settings: BarSettings,
    pub hyprland: Option<Arc<HyprlandService>>,
    pub config: Arc<ConfigService>,
}

#[derive(Debug)]
pub(crate) enum WorkspacesMsg {
    WorkspaceClicked(String),
    ScrollUp,
    ScrollDown,
}

#[derive(Debug)]
pub(crate) enum WorkspacesCmd {
    WorkspacesChanged,
    ClientsChanged,
    ActiveWorkspaceChanged(String),
    MonitorFocused {
        monitor: String,
        workspace_id: String,
    },
    TitleChanged,
    ConfigChanged,
    HyprlandConfigReloaded,
    UrgentWindow(Address),
    WindowFocused(Address),
    BlinkTick,
    WorkspaceRulesLoaded(HashMap<String, String>),
}
