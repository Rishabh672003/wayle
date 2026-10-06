use std::sync::Arc;

use futures::{
    StreamExt,
    stream::{select, select_all},
};
use relm4::prelude::FactorySender;
use tokio_util::sync::CancellationToken;
use wayle_systray::core::item::TrayItem;

use super::{SystrayItem, SystrayItemMsg};

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

pub(super) fn spawn_icon_watcher(
    sender: &FactorySender<SystrayItem>,
    item: &Arc<TrayItem>,
    cancel_token: CancellationToken,
) {
    let icon_name = item.icon_name.watch().skip(1).map(|_| ());
    let icon_pixmap = item.icon_pixmap.watch().skip(1).map(|_| ());
    let status = item.status.watch().skip(1).map(|_| ());
    let attention_name = item.attention_icon_name.watch().skip(1).map(|_| ());
    let attention_pixmap = item.attention_icon_pixmap.watch().skip(1).map(|_| ());
    let stream = select_all([
        icon_name.boxed(),
        icon_pixmap.boxed(),
        status.boxed(),
        attention_name.boxed(),
        attention_pixmap.boxed(),
    ]);
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
