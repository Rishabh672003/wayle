use std::sync::Arc;

use wayle_systray::{core::item::TrayItem, types::item::Status};

use super::{SystrayModule, helpers::is_blacklisted, item::SystrayItemInit};

impl SystrayModule {
    pub(super) fn update_items(&mut self, items: Vec<Arc<TrayItem>>) {
        let config = &self.config.config().modules.systray;

        let mut guard = self.items.guard();
        guard.clear();

        for item in items {
            if item.status.get() == Status::Passive || is_blacklisted(&item, config) {
                continue;
            }
            guard.push_back(SystrayItemInit {
                item,
                config: self.config.clone(),
            });
        }

        self.visible.set(!guard.is_empty());
    }
}
