pub(crate) mod messages;
mod methods;
mod toggles;
mod watchers;

use std::{sync::Arc, time::Duration};

use gtk::{pango::EllipsizeMode, prelude::*};
use gtk4_layer_shell::{KeyboardMode, LayerShell};
use relm4::{gtk, prelude::*};
use tracing::debug;
use wayle_audio::AudioService;
use wayle_battery::{BatteryService, types::DeviceState};
use wayle_brightness::BrightnessService;
use wayle_config::ConfigService;
use wayle_widgets::WatcherToken;

pub(crate) use self::messages::OsdInit;
use self::{
    messages::{OsdCmd, OsdEvent},
    methods::{
        event_fraction, event_icon, event_label, event_slider_label, event_value, is_slider,
        is_toggle, osd_classes,
    },
};

const BRIGHTNESS_ICON: &str = "ld-sun-symbolic";
const BATTERY_ICON: &str = "md-battery_android_frame_full-symbolic";
const BATTERY_CHARGING_ICON: &str = "md-battery_android_frame_bolt-symbolic";
const BATTERY_LOW_ICON: &str = "md-battery_android_alert-symbolic";

/// Battery percentage at or below which a low-battery OSD is shown (transient).
const BATTERY_LOW_THRESHOLD: f64 = 20.0;

/// How long the transient low-battery OSD stays up. Longer than the shared
/// `osd.duration` (used for volume/brightness) since it's an alert, not feedback.
const BATTERY_LOW_OSD_DURATION_MS: u32 = 10000;

/// Battery percentage at or below which a critical OSD is shown. Unlike the
/// low one it is sticky: it stays until the battery starts charging.
const BATTERY_CRITICAL_THRESHOLD: f64 = 5.0;

pub(crate) struct Osd {
    config: Arc<ConfigService>,
    audio: Option<Arc<AudioService>>,
    brightness: Option<Arc<BrightnessService>>,
    battery: Option<Arc<BatteryService>>,
    dismiss_id: u32,
    ready: bool,
    device_watcher: WatcherToken,
    input_device_watcher: WatcherToken,
    brightness_watcher: WatcherToken,

    current_event: Option<OsdEvent>,
    last_volume: Option<(u32, bool)>,
    last_input_volume: Option<(u32, bool)>,
    last_brightness: Option<u32>,
    last_battery_state: Option<DeviceState>,
    battery_low_shown: bool,
    /// While set, the critical-battery OSD is kept on screen (and re-shown if
    /// another OSD transiently replaces it) until the battery starts charging.
    battery_critical_event: Option<OsdEvent>,
}

#[allow(clippy::needless_borrow)]
#[relm4::component(pub(crate))]
impl Component for Osd {
    type Init = OsdInit;
    type Input = ();
    type Output = ();
    type CommandOutput = OsdCmd;

    view! {
        #[root]
        gtk::Window {
            set_decorated: false,
            add_css_class: "osd-host",
            set_default_size: (1, 1),
            set_visible: false,

            #[name = "osd_container"]
            gtk::Box {
                set_orientation: gtk::Orientation::Vertical,

                #[watch]
                set_css_classes: &osd_classes(&model),

                #[name = "slider_header"]
                gtk::Box {
                    add_css_class: "osd-header",

                    #[watch]
                    set_visible: is_slider(&model.current_event),

                    #[name = "slider_icon"]
                    gtk::Image {
                        add_css_class: "osd-icon",
                        set_valign: gtk::Align::Center,

                        #[watch]
                        set_icon_name: event_icon(&model.current_event),
                    },

                    #[name = "slider_label"]
                    gtk::Label {
                        add_css_class: "osd-label",
                        set_hexpand: true,
                        set_halign: gtk::Align::Start,
                        set_valign: gtk::Align::Center,
                        set_ellipsize: EllipsizeMode::End,

                        #[watch]
                        set_label: &event_slider_label(&model.current_event),
                    },

                    #[name = "value"]
                    gtk::Label {
                        add_css_class: "osd-value",
                        set_valign: gtk::Align::Center,

                        #[watch]
                        set_label: &event_value(&model.current_event),
                    },
                },

                #[name = "toggle_header"]
                gtk::Box {
                    add_css_class: "osd-header",

                    #[watch]
                    set_visible: is_toggle(&model.current_event),

                    #[name = "toggle_icon"]
                    gtk::Image {
                        add_css_class: "osd-icon",
                        set_valign: gtk::Align::Center,

                        #[watch]
                        set_icon_name: event_icon(&model.current_event),
                    },

                    #[name = "toggle_label"]
                    gtk::Label {
                        add_css_class: "osd-label",
                        set_valign: gtk::Align::Center,

                        #[watch]
                        set_label: &event_label(&model.current_event),
                    },
                },

                #[name = "bar"]
                gtk::ProgressBar {
                    add_css_class: "osd-bar",

                    #[watch]
                    set_fraction: event_fraction(&model.current_event),

                    #[watch]
                    set_visible: is_slider(&model.current_event),
                },
            },
        }
    }

    fn init(
        init: Self::Init,
        root: Self::Root,
        sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        root.init_layer_shell();
        root.set_keyboard_mode(KeyboardMode::None);
        root.set_namespace(Some("wayle-osd"));

        let model = Self {
            config: init.config.clone(),
            audio: init.audio.clone(),
            brightness: init.brightness.clone(),
            battery: init.battery.clone(),
            dismiss_id: 0,
            ready: false,
            device_watcher: WatcherToken::new(),
            input_device_watcher: WatcherToken::new(),
            brightness_watcher: WatcherToken::new(),
            current_event: None,
            last_volume: None,
            last_input_volume: None,
            last_brightness: None,
            last_battery_state: None,
            battery_low_shown: false,
            battery_critical_event: None,
        };

        model.apply_position(&root);
        model.apply_layer(&root);

        sender.oneshot_command(async {
            tokio::time::sleep(Duration::from_millis(500)).await;
            OsdCmd::Ready
        });

        let widgets = view_output!();

        watchers::spawn(
            &sender,
            &init.config,
            &init.audio,
            &init.brightness,
            &init.battery,
        );

        ComponentParts { model, widgets }
    }

    fn update_cmd(&mut self, msg: OsdCmd, sender: ComponentSender<Self>, root: &Self::Root) {
        match msg {
            OsdCmd::Ready => {
                self.ready = true;
            }

            OsdCmd::Dismiss(dismiss_id) => {
                if dismiss_id == self.dismiss_id {
                    // A sticky critical-battery OSD outlives transient ones:
                    // fall back to it instead of hiding.
                    if let Some(event) = self.battery_critical_event.clone() {
                        self.current_event = Some(event);
                        root.set_visible(true);
                    } else {
                        root.set_visible(false);
                        debug!("OSD dismissed");
                    }
                }
            }

            OsdCmd::ConfigChanged => {
                self.apply_position(root);
                self.apply_layer(root);
            }

            OsdCmd::DeviceChanged(device) => {
                self.handle_device_changed(device, &sender);
            }

            OsdCmd::VolumeChanged => {
                self.handle_volume_changed(&sender, root);
            }

            OsdCmd::BrightnessDeviceChanged(device) => {
                self.handle_brightness_device_changed(device, &sender);
            }

            OsdCmd::BrightnessChanged => {
                self.handle_brightness_changed(&sender, root);
            }

            OsdCmd::BatteryChanged => {
                self.handle_battery_changed(&sender, root);
            }

            OsdCmd::InputDeviceChanged(device) => {
                self.handle_input_device_changed(device, &sender);
            }

            OsdCmd::InputVolumeChanged => {
                self.handle_input_volume_changed(&sender, root);
            }

            OsdCmd::ToggleChanged(toggle) => {
                self.handle_toggle_changed(toggle, &sender, root);
            }
        }
    }
}
