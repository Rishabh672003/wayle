use std::sync::Arc;

use futures::{StreamExt, stream::select_all};
use relm4::ComponentSender;
use wayle_config::{ConfigProperty, ConfigService};
use wayle_systray::SystemTrayService;
use wayle_widgets::watch;

use super::{SystrayCmd, SystrayModule};

pub(super) fn spawn_watchers(
    sender: &ComponentSender<SystrayModule>,
    is_vertical: &ConfigProperty<bool>,
    systray: &Arc<SystemTrayService>,
    config_service: &Arc<ConfigService>,
) {
    let full_config = config_service.config();
    let systray_config = &full_config.modules.systray;
    let bar_config = &full_config.bar;

    let items = systray.items.clone();
    let blacklist = systray_config.blacklist.clone();
    let overrides = systray_config.overrides.clone();
    watch!(
        sender,
        [items.watch(), blacklist.watch(), overrides.watch()],
        |out| {
            let _ = out.send(SystrayCmd::ItemsChanged(items.get()));
        }
    );

    // Passive items are hidden, so refilter whenever any item's status flips.
    let items = systray.items.clone();
    sender.command(move |out, shutdown| async move {
        let mut list = items.watch();
        let watch_statuses = async {
            let Some(mut current) = list.next().await else {
                return;
            };
            loop {
                let mut statuses = select_all(
                    current
                        .iter()
                        .map(|item| Box::pin(item.status.watch().skip(1))),
                );
                tokio::select! {
                    next = list.next() => match next {
                        Some(next) => current = next,
                        None => return,
                    },
                    Some(_) = statuses.next() => {
                        let _ = out.send(SystrayCmd::ItemsChanged(items.get()));
                    }
                }
            }
        };
        tokio::select! {
            () = shutdown.wait() => {}
            () = watch_statuses => {}
        }
    });

    let item_gap = systray_config.item_gap.clone();
    let icon_scale = systray_config.icon_scale.clone();
    let internal_padding = systray_config.internal_padding.clone();
    let bar_scale = bar_config.scale.clone();
    watch!(
        sender,
        [
            item_gap.watch(),
            icon_scale.watch(),
            internal_padding.watch(),
            bar_scale.watch()
        ],
        |out| {
            let _ = out.send(SystrayCmd::StylingChanged);
        }
    );

    let is_vertical = is_vertical.clone();
    watch!(sender, [is_vertical.watch()], |out| {
        let _ = out.send(SystrayCmd::OrientationChanged(is_vertical.get()));
    });
}
