use std::sync::Arc;

use futures::{StreamExt, stream::select};
use relm4::prelude::FactorySender;
use tokio_util::sync::CancellationToken;
use tracing::warn;
use wayle_systray::{core::item::TrayItem, types::item::Tooltip};
use zbus::proxy::CacheProperties;

use super::{SystrayItem, SystrayItemMsg};

/// Minimal StatusNotifierItem proxy: just the `ToolTip` property, read
/// uncached so each call hits the app live.
#[zbus::proxy(
    interface = "org.kde.StatusNotifierItem",
    default_path = "/StatusNotifierItem"
)]
trait StatusNotifierItem {
    #[zbus(property)]
    fn tool_tip(&self) -> zbus::Result<(String, Vec<(i32, i32, Vec<u8>)>, String, String)>;
}

pub(super) fn spawn_menu_watcher(
    sender: &FactorySender<SystrayItem>,
    item: &Arc<TrayItem>,
    cancel_token: CancellationToken,
) {
    let stream = item.menu.watch().skip(1);
    let sender = sender.clone();

    relm4::spawn_local(async move {
        futures::pin_mut!(stream);

        loop {
            tokio::select! {
                () = cancel_token.cancelled() => break,
                result = stream.next() => {
                    if result.is_none() {
                        break;
                    }
                    sender.input(SystrayItemMsg::MenuUpdated);
                }
            }
        }
    });
}

/// Re-queries the GTK tooltip whenever the item's tooltip or title changes,
/// so a visible tooltip updates live instead of freezing on the first read.
pub(super) fn spawn_tooltip_watcher(
    sender: &FactorySender<SystrayItem>,
    item: &Arc<TrayItem>,
    cancel_token: CancellationToken,
) {
    let tooltip = item.tooltip.watch().skip(1).map(|_| ());
    let title = item.title.watch().skip(1).map(|_| ());
    let stream = select(tooltip, title);
    let sender = sender.clone();

    relm4::spawn_local(async move {
        futures::pin_mut!(stream);

        loop {
            tokio::select! {
                () = cancel_token.cancelled() => break,
                result = stream.next() => {
                    if result.is_none() {
                        break;
                    }
                    sender.input(SystrayItemMsg::TooltipUpdated);
                }
            }
        }
    });
}

/// Keeps the tooltip fresh. wayle-systray only refreshes `tooltip` on D-Bus
/// `PropertiesChanged`, which most SNI apps (qBittorrent, Qt) never emit —
/// they emit the `NewToolTip` signal instead. On each such signal we re-read
/// the `ToolTip` property and push it into the item's `tooltip`, which
/// `spawn_tooltip_watcher` already reacts to.
// ponytail: one session connection + proxy per item; share a connection if item count ever grows.
pub(super) fn spawn_tooltip_refresher(item: &Arc<TrayItem>, cancel_token: CancellationToken) {
    let item = item.clone();

    tokio::spawn(async move {
        let signals = match item.new_tool_tip_signal().await {
            Ok(signals) => signals,
            Err(error) => {
                warn!(id = %item.id.get(), %error, "tooltip refresher: no NewToolTip signal");
                return;
            }
        };

        let bus_name = item.bus_name.get();
        let (service, path) = match bus_name.find('/') {
            Some(pos) => (&bus_name[..pos], &bus_name[pos..]),
            None => (bus_name.as_str(), "/StatusNotifierItem"),
        };

        let proxy = async {
            let connection = zbus::Connection::session().await?;
            StatusNotifierItemProxy::builder(&connection)
                .destination(service)?
                .path(path)?
                .cache_properties(CacheProperties::No)
                .build()
                .await
        }
        .await;
        let proxy = match proxy {
            Ok(proxy) => proxy,
            Err(error) => {
                warn!(id = %item.id.get(), %error, "tooltip refresher: proxy build failed");
                return;
            }
        };

        futures::pin_mut!(signals);
        loop {
            tokio::select! {
                () = cancel_token.cancelled() => break,
                signal = signals.next() => {
                    if signal.is_none() {
                        break;
                    }
                    if let Ok(raw) = proxy.tool_tip().await {
                        item.tooltip.set(Tooltip::from(raw));
                    }
                }
            }
        }
    });
}

pub(super) fn spawn_icon_watcher(
    sender: &FactorySender<SystrayItem>,
    item: &Arc<TrayItem>,
    cancel_token: CancellationToken,
) {
    let icon_name = item.icon_name.watch().skip(1).map(|_| ());
    let icon_pixmap = item.icon_pixmap.watch().skip(1).map(|_| ());
    let stream = select(icon_name, icon_pixmap);
    let sender = sender.clone();

    relm4::spawn_local(async move {
        futures::pin_mut!(stream);

        loop {
            tokio::select! {
                () = cancel_token.cancelled() => break,
                result = stream.next() => {
                    if result.is_none() {
                        break;
                    }
                    sender.input(SystrayItemMsg::IconUpdated);
                }
            }
        }
    });
}
