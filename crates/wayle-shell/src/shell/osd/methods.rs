use std::{sync::Arc, time::Duration};

use gtk4_layer_shell::{Edge, LayerShell};
use relm4::{ComponentSender, gtk, gtk::prelude::*};
use wayle_audio::core::device::{input::InputDevice, output::OutputDevice};
use wayle_battery::types::DeviceState;
use wayle_brightness::BacklightDevice;
use wayle_config::schemas::osd::{OsdMonitor, OsdPosition};

use super::{
    BATTERY_CHARGING_ICON, BATTERY_CRITICAL_THRESHOLD, BATTERY_ICON, BATTERY_LOW_ICON,
    BATTERY_LOW_OSD_DURATION_MS, BATTERY_LOW_THRESHOLD, BRIGHTNESS_ICON, Osd, messages,
    messages::{OsdCmd, OsdEvent},
    watchers,
};
use crate::{
    i18n::t,
    shell::helpers::layer_shell::{
        apply_layer as apply_window_layer, apply_monitor_by_connector, apply_primary_monitor,
        reset_anchors,
    },
};

impl Osd {
    pub(super) fn show_event(
        &mut self,
        event: OsdEvent,
        sender: &ComponentSender<Self>,
        root: &gtk::Window,
    ) {
        let duration = self.config.config().osd.duration.get();
        self.show_event_for(event, duration, sender, root);
    }

    /// Like `show_event` but with an explicit dismiss duration (ms).
    pub(super) fn show_event_for(
        &mut self,
        event: OsdEvent,
        duration_ms: u32,
        sender: &ComponentSender<Self>,
        root: &gtk::Window,
    ) {
        if !self.ready {
            return;
        }

        self.current_event = Some(event);
        self.dismiss_id = self.dismiss_id.wrapping_add(1);

        root.set_visible(true);

        Self::schedule_dismiss(sender, duration_ms, self.dismiss_id);
    }

    pub(super) fn handle_device_changed(
        &mut self,
        device: Option<Arc<OutputDevice>>,
        sender: &ComponentSender<Self>,
    ) {
        let token = self.device_watcher.reset();

        if let Some(device) = &device {
            watchers::spawn_device_watchers(sender, device, token);
        }
    }

    pub(super) fn handle_volume_changed(
        &mut self,
        sender: &ComponentSender<Self>,
        root: &gtk::Window,
    ) {
        let Some(audio) = &self.audio else {
            return;
        };

        let Some(device) = audio.default_output.get() else {
            return;
        };

        let percentage = device.volume.get().average_percentage();
        let muted = device.muted.get();
        let rounded = percentage.round() as u32;

        let snapshot = (rounded, muted);

        if self.last_volume == Some(snapshot) {
            return;
        }

        self.last_volume = Some(snapshot);

        let description = device.description.get();
        let icon = volume_icon(percentage, muted);

        let event = OsdEvent::Slider {
            label: description,
            icon: icon.to_string(),
            percentage,
            muted,
        };

        self.show_event(event, sender, root);
    }

    pub(super) fn handle_brightness_device_changed(
        &mut self,
        device: Option<Arc<BacklightDevice>>,
        sender: &ComponentSender<Self>,
    ) {
        let token = self.brightness_watcher.reset();

        if let Some(device) = &device {
            watchers::spawn_brightness_watcher(sender, device, token);
        }
    }

    pub(super) fn handle_brightness_changed(
        &mut self,
        sender: &ComponentSender<Self>,
        root: &gtk::Window,
    ) {
        let Some(brightness) = &self.brightness else {
            return;
        };

        let Some(device) = brightness.primary.get() else {
            return;
        };

        let percentage = device.percentage().value();
        let rounded = percentage.round() as u32;

        if self.last_brightness == Some(rounded) {
            return;
        }

        self.last_brightness = Some(rounded);

        let event = OsdEvent::Slider {
            label: device.name.to_string(),
            icon: BRIGHTNESS_ICON.to_string(),
            percentage,
            muted: false,
        };

        self.show_event(event, sender, root);
    }

    pub(super) fn handle_battery_changed(
        &mut self,
        sender: &ComponentSender<Self>,
        root: &gtk::Window,
    ) {
        let Some(battery) = &self.battery else {
            return;
        };

        if !self.config.config().osd.battery.get() {
            return;
        }

        let device = &battery.device;
        let state = device.state.get();
        let percentage = device.percentage.get();

        let previous_state = self.last_battery_state.replace(state);
        let discharging = matches!(
            state,
            DeviceState::Discharging | DeviceState::PendingDischarge
        );
        let charging =
            matches!(state, DeviceState::Charging | DeviceState::FullyCharged);

        // Reset the low latch once charging or recovered above threshold.
        if charging || percentage > BATTERY_LOW_THRESHOLD {
            self.battery_low_shown = false;
        }

        // Release the sticky critical OSD once charging (or recovered).
        if charging || percentage > BATTERY_CRITICAL_THRESHOLD {
            self.battery_critical_event = None;
        }

        // Skip the first reading so we don't pop up on startup.
        let Some(previous_state) = previous_state else {
            return;
        };

        // State transitions take priority (transient popups).
        if state != previous_state {
            let transition = match state {
                DeviceState::Charging | DeviceState::PendingCharge => {
                    Some(("Charging", BATTERY_CHARGING_ICON))
                }
                DeviceState::Discharging | DeviceState::PendingDischarge => {
                    Some(("On Battery", BATTERY_ICON))
                }
                DeviceState::FullyCharged => Some(("Fully Charged", BATTERY_CHARGING_ICON)),
                _ => None,
            };

            if let Some((label, icon)) = transition {
                self.show_event(
                    Self::battery_event(label, icon, percentage),
                    sender,
                    root,
                );
                return;
            }
        }

        if !discharging {
            return;
        }

        // Critical: sticky, shown once and kept until charging.
        if percentage <= BATTERY_CRITICAL_THRESHOLD && self.battery_critical_event.is_none() {
            self.battery_low_shown = true;
            let event = Self::battery_event("Critical Battery", BATTERY_LOW_ICON, percentage);
            self.show_sticky_battery(event, root);
            return;
        }

        // Low: transient, shown once per drain below the threshold.
        if percentage <= BATTERY_LOW_THRESHOLD
            && !self.battery_low_shown
            && self.battery_critical_event.is_none()
        {
            self.battery_low_shown = true;
            self.show_event_for(
                Self::battery_event("Low Battery", BATTERY_LOW_ICON, percentage),
                BATTERY_LOW_OSD_DURATION_MS,
                sender,
                root,
            );
        }
    }

    fn battery_event(label: &str, icon: &str, percentage: f64) -> OsdEvent {
        OsdEvent::Slider {
            label: label.to_string(),
            icon: icon.to_string(),
            percentage,
            muted: false,
        }
    }

    /// Shows an OSD that stays up (no auto-dismiss). It is re-asserted after
    /// any transient OSD dismisses, and cleared in `handle_battery_changed`.
    fn show_sticky_battery(&mut self, event: OsdEvent, root: &gtk::Window) {
        // Not ready yet: don't latch, so the next battery tick retries.
        if !self.ready {
            return;
        }

        self.battery_critical_event = Some(event.clone());
        self.current_event = Some(event);
        self.dismiss_id = self.dismiss_id.wrapping_add(1);
        root.set_visible(true);
    }

    pub(super) fn handle_input_device_changed(
        &mut self,
        device: Option<Arc<InputDevice>>,
        sender: &ComponentSender<Self>,
    ) {
        let token = self.input_device_watcher.reset();

        if let Some(device) = &device {
            watchers::spawn_input_device_watchers(sender, device, token);
        }
    }

    pub(super) fn handle_input_volume_changed(
        &mut self,
        sender: &ComponentSender<Self>,
        root: &gtk::Window,
    ) {
        let Some(audio) = &self.audio else {
            return;
        };

        let Some(device) = audio.default_input.get() else {
            return;
        };

        let percentage = device.volume.get().average_percentage();
        let muted = device.muted.get();
        let rounded = percentage.round() as u32;

        let snapshot = (rounded, muted);

        if self.last_input_volume == Some(snapshot) {
            return;
        }

        self.last_input_volume = Some(snapshot);

        let description = device.description.get();

        let icon = if muted {
            "ld-mic-off-symbolic"
        } else {
            "ld-mic-symbolic"
        };

        let event = OsdEvent::Slider {
            label: description,
            icon: icon.to_string(),
            percentage,
            muted,
        };

        self.show_event(event, sender, root);
    }

    pub(super) fn handle_toggle_changed(
        &mut self,
        toggle: messages::ToggleEvent,
        sender: &ComponentSender<Self>,
        root: &gtk::Window,
    ) {
        let (label, icon) = match toggle.key {
            messages::ToggleKey::CapsLock => (t!("osd-caps-lock"), "ld-a-large-small-symbolic"),
            messages::ToggleKey::NumLock => (t!("osd-num-lock"), "ld-hash-symbolic"),
            messages::ToggleKey::ScrollLock => (t!("osd-scroll-lock"), "ld-arrow-up-down-symbolic"),
        };

        let event = OsdEvent::Toggle {
            label,
            icon: icon.to_string(),
            active: toggle.active,
        };

        self.show_event(event, sender, root);
    }

    pub(super) fn apply_position(&self, root: &gtk::Window) {
        let config = self.config.config();
        let osd_config = &config.osd;
        let position = osd_config.position.get();
        let scale = config.styling.scale.get().value();
        let margin = (osd_config.margin.get().value() * scale) as i32;

        reset_anchors(root);

        match position {
            OsdPosition::TopLeft => {
                root.set_anchor(Edge::Top, true);
                root.set_anchor(Edge::Left, true);
                root.set_margin(Edge::Top, margin);
                root.set_margin(Edge::Left, margin);
            }

            OsdPosition::Top => {
                root.set_anchor(Edge::Top, true);
                root.set_margin(Edge::Top, margin);
            }

            OsdPosition::TopRight => {
                root.set_anchor(Edge::Top, true);
                root.set_anchor(Edge::Right, true);
                root.set_margin(Edge::Top, margin);
                root.set_margin(Edge::Right, margin);
            }

            OsdPosition::Right => {
                root.set_anchor(Edge::Right, true);
                root.set_margin(Edge::Right, margin);
            }

            OsdPosition::BottomRight => {
                root.set_anchor(Edge::Bottom, true);
                root.set_anchor(Edge::Right, true);
                root.set_margin(Edge::Bottom, margin);
                root.set_margin(Edge::Right, margin);
            }

            OsdPosition::Bottom => {
                root.set_anchor(Edge::Bottom, true);
                root.set_margin(Edge::Bottom, margin);
            }

            OsdPosition::BottomLeft => {
                root.set_anchor(Edge::Bottom, true);
                root.set_anchor(Edge::Left, true);
                root.set_margin(Edge::Bottom, margin);
                root.set_margin(Edge::Left, margin);
            }

            OsdPosition::Left => {
                root.set_anchor(Edge::Left, true);
                root.set_margin(Edge::Left, margin);
            }
        }

        let monitor = osd_config.monitor.get();

        match &monitor {
            OsdMonitor::Primary => apply_primary_monitor(root),
            OsdMonitor::Connector(name) => {
                apply_monitor_by_connector(root, name);
            }
        }
    }

    pub(super) fn apply_layer(&self, root: &gtk::Window) {
        let configured = self.config.config().osd.layer.get();
        apply_window_layer(root, configured, &self.config);
    }

    pub(super) fn schedule_dismiss(
        sender: &ComponentSender<Osd>,
        duration_ms: u32,
        dismiss_id: u32,
    ) {
        sender.oneshot_command(async move {
            tokio::time::sleep(Duration::from_millis(duration_ms as u64)).await;
            OsdCmd::Dismiss(dismiss_id)
        });
    }
}

pub(super) fn osd_classes(model: &Osd) -> Vec<&'static str> {
    let mut classes = vec!["osd"];

    if model
        .current_event
        .as_ref()
        .is_some_and(|event| matches!(event, OsdEvent::Slider { muted: true, .. }))
    {
        classes.push("muted");
    }

    if model
        .current_event
        .as_ref()
        .is_some_and(|event| matches!(event, OsdEvent::Toggle { active: false, .. }))
    {
        classes.push("toggle-off");
    }

    if model.config.config().osd.border.get() {
        classes.push("bordered");
    }

    classes
}

pub(super) fn is_slider(event: &Option<OsdEvent>) -> bool {
    event
        .as_ref()
        .is_some_and(|event| matches!(event, OsdEvent::Slider { .. }))
}

pub(super) fn is_toggle(event: &Option<OsdEvent>) -> bool {
    event
        .as_ref()
        .is_some_and(|event| matches!(event, OsdEvent::Toggle { .. }))
}

pub(super) fn event_icon(event: &Option<OsdEvent>) -> Option<&str> {
    match event {
        Some(OsdEvent::Slider { icon, .. }) | Some(OsdEvent::Toggle { icon, .. }) => {
            Some(icon.as_str())
        }
        None => None,
    }
}

pub(super) fn event_slider_label(event: &Option<OsdEvent>) -> String {
    match event {
        Some(OsdEvent::Slider { label, .. }) => label.clone(),
        _ => String::new(),
    }
}

pub(super) fn event_label(event: &Option<OsdEvent>) -> String {
    match event {
        Some(OsdEvent::Slider { label, .. }) => label.clone(),

        Some(OsdEvent::Toggle {
            label,
            active: true,
            ..
        }) => t!("osd-toggle-on", label = label.clone()),

        Some(OsdEvent::Toggle {
            label,
            active: false,
            ..
        }) => t!("osd-toggle-off", label = label.clone()),

        None => String::new(),
    }
}

pub(super) fn event_value(event: &Option<OsdEvent>) -> String {
    match event {
        Some(OsdEvent::Slider { percentage, .. }) => format!("{}%", percentage.round() as u32),
        _ => String::new(),
    }
}

pub(super) fn event_fraction(event: &Option<OsdEvent>) -> f64 {
    match event {
        Some(OsdEvent::Slider { percentage, .. }) => (*percentage / 100.0).clamp(0.0, 1.0),
        _ => 0.0,
    }
}

fn volume_icon(percentage: f64, muted: bool) -> &'static str {
    if muted || percentage <= 0.0 {
        "ld-volume-x-symbolic"
    } else if percentage < 34.0 {
        "ld-volume-symbolic"
    } else if percentage < 67.0 {
        "ld-volume-1-symbolic"
    } else {
        "ld-volume-2-symbolic"
    }
}
