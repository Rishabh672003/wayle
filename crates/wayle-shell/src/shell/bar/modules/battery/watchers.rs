use std::sync::Arc;

use relm4::ComponentSender;
use wayle_battery::{BatteryService, types::DeviceState};
use wayle_config::schemas::{modules::BatteryConfig, styling::evaluate_thresholds};
use wayle_widgets::watch;

use super::{
    BatteryModule,
    helpers::{IconContext, format_label, select_icon},
    messages::BatteryCmd,
};
use crate::shell::bar::dropdowns::format_time_remaining;

pub(super) fn spawn_watchers(
    sender: &ComponentSender<BatteryModule>,
    config: &BatteryConfig,
    battery: &Arc<BatteryService>,
) {
    let level_icons = config.level_icons.clone();
    let charging_icon = config.charging_icon.clone();
    let alert_icon = config.alert_icon.clone();
    let format = config.format.clone();
    let thresholds = config.thresholds.clone();

    let device = battery.device.clone();

    let percentage_stream = device.percentage.watch();
    let state_stream = device.state.watch();
    let is_present_stream = device.is_present.watch();
    let time_to_empty_stream = device.time_to_empty.watch();
    let time_to_full_stream = device.time_to_full.watch();
    let level_icons_stream = level_icons.watch();
    let charging_icon_stream = charging_icon.watch();
    let alert_icon_stream = alert_icon.watch();
    let format_stream = format.watch();

    let thresholds_watch = thresholds.clone();
    let device_watch = device.clone();
    watch!(sender, [thresholds_watch.watch()], |out| {
        let percentage = device_watch.percentage.get();
        let colors = evaluate_thresholds(percentage, &thresholds_watch.get());
        let _ = out.send(BatteryCmd::UpdateThresholdColors(colors));
    });

    watch!(
        sender,
        [
            percentage_stream,
            state_stream,
            is_present_stream,
            time_to_empty_stream,
            time_to_full_stream,
            level_icons_stream,
            charging_icon_stream,
            alert_icon_stream,
            format_stream
        ],
        |out| {
            let percentage = device.percentage.get();
            let state = device.state.get();
            let is_present = device.is_present.get();

            let label = format_label(&format.get(), percentage, is_present);
            let _ = out.send(BatteryCmd::UpdateLabel(label));

            let charging = matches!(state, DeviceState::Charging | DeviceState::PendingCharge);
            let seconds = if charging {
                device.time_to_full.get()
            } else {
                device.time_to_empty.get()
            };
            let tooltip = format_time_remaining(seconds, charging);
            let _ = out.send(BatteryCmd::UpdateTooltip(tooltip));

            let level_icons_val = level_icons.get();
            let charging_icon_val = charging_icon.get();
            let alert_icon_val = alert_icon.get();
            let icon = select_icon(&IconContext {
                percentage,
                state,
                is_present,
                level_icons: &level_icons_val,
                charging_icon: &charging_icon_val,
                alert_icon: &alert_icon_val,
            });
            let _ = out.send(BatteryCmd::UpdateIcon(icon));

            let colors = evaluate_thresholds(percentage, &thresholds.get());
            let _ = out.send(BatteryCmd::UpdateThresholdColors(colors));
        }
    );
}
