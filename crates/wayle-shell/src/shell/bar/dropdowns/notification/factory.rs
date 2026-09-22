use relm4::prelude::*;

use super::{NotificationDropdown, messages::NotificationDropdownInit};
use crate::shell::{
    bar::dropdowns::{DropdownFactory, DropdownInstance, require_service},
    services::ShellServices,
};

pub(crate) struct Factory;

impl DropdownFactory for Factory {
    fn create(services: &ShellServices) -> Option<DropdownInstance> {
        let notification = require_service(
            "notification",
            "notification",
            services.notification.clone(),
        )?;
        let config = services.config.clone();
        let history = services.shell_ipc.state().notification_history;

        let init = NotificationDropdownInit {
            notification,
            config,
            history,
        };
        let controller = NotificationDropdown::builder().launch(init).detach();

        let popover = controller.widget().clone();
        Some(DropdownInstance::new(popover, Box::new(controller)))
    }
}
