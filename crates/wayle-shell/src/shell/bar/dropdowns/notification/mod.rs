mod factory;
mod helpers;
mod messages;
mod methods;
mod notification_group;
mod notification_item;
mod watchers;

use std::sync::Arc;

use gtk::prelude::*;
use relm4::{gtk, prelude::*};
use wayle_config::ConfigService;
use wayle_core::Property;
use wayle_notification::{NotificationService, core::notification::Notification};
use wayle_widgets::prelude::*;

pub(super) use self::factory::Factory;
use self::{
    messages::{NotificationDropdownCmd, NotificationDropdownInit, NotificationDropdownMsg},
    notification_group::{
        NotificationGroup,
        messages::{NotificationGroupInput, NotificationGroupOutput},
    },
    notification_item::{NotificationItem, messages::NotificationItemOutput},
};
use crate::{i18n::t, shell::bar::dropdowns::scaled_dimension};

const BASE_WIDTH: f32 = 425.0;
const BASE_HEIGHT: f32 = 725.0;

pub(crate) struct NotificationDropdown {
    notification: Arc<NotificationService>,
    config: Arc<ConfigService>,

    scaled_width: i32,
    scaled_height: i32,

    dnd: bool,
    has_notifications: bool,
    showing_history: bool,

    history: Property<Vec<Arc<Notification>>>,

    groups: FactoryVecDeque<NotificationGroup>,
    history_items: FactoryVecDeque<NotificationItem>,
}

#[relm4::component(pub(crate))]
impl Component for NotificationDropdown {
    type Init = NotificationDropdownInit;
    type Input = NotificationDropdownMsg;
    type Output = ();
    type CommandOutput = NotificationDropdownCmd;

    view! {
        #[root]
        gtk::Popover {
            set_css_classes: &["dropdown", "notification-dropdown"],
            set_has_arrow: false,
            #[watch]
            set_width_request: model.scaled_width,
            #[watch]
            set_height_request: model.scaled_height,

            #[template]
            Dropdown {
                set_overflow: gtk::Overflow::Hidden,

                #[template]
                DropdownHeader {
                    #[template_child]
                    icon {
                        set_visible: true,
                        #[watch]
                        set_icon_name: Some(
                            if model.dnd {
                                "ld-bell-off-symbolic"
                            } else {
                                "ld-bell-symbolic"
                            }
                        ),
                    },
                    #[template_child]
                    label {
                        set_label: &t!("notification-dropdown-title"),
                    },
                    #[template_child]
                    actions {
                        #[template]
                        GhostButton {
                            add_css_class: "notification-dropdown-clear-all",
                            #[watch]
                            set_visible: model.has_notifications && !model.showing_history,
                            connect_clicked[sender] => move |_| {
                                sender.input(NotificationDropdownMsg::ClearAll);
                            },
                            #[template_child]
                            label {
                                set_label: &t!("notification-dropdown-clear-all"),
                            },
                        },

                        #[template]
                        GhostIconButton {
                            add_css_class: "notification-dropdown-history-toggle",
                            #[watch]
                            set_icon_name: if model.showing_history {
                                "ld-bell-symbolic"
                            } else {
                                "ld-clock-symbolic"
                            },
                            set_tooltip_text: Some(&t!("notification-dropdown-history")),
                            connect_clicked[sender] => move |_| {
                                sender.input(NotificationDropdownMsg::ToggleHistory);
                            },
                        },
                    },
                },

                gtk::Box {
                    add_css_class: "notification-dropdown-dnd-row",

                    gtk::Label {
                        add_css_class: "notification-dropdown-dnd-label",
                        set_hexpand: true,
                        set_halign: gtk::Align::Start,
                        set_label: &t!("notification-dropdown-dnd-label"),
                    },

                    #[template]
                    Switch {
                        #[watch]
                        #[block_signal(dnd_toggle)]
                        set_active: model.dnd,
                        connect_state_set[sender] => move |switch, active| {
                            sender.input(NotificationDropdownMsg::DndToggled(active));
                            switch.set_state(active);
                            gtk::glib::Propagation::Stop
                        } @dnd_toggle,
                    },
                },

                #[template]
                DropdownContent {
                    add_css_class: "notification-dropdown-content",

                    #[template]
                    EmptyState {
                        #[watch]
                        set_visible: !model.showing_history && !model.has_notifications,
                        #[template_child]
                        icon {
                            #[watch]
                            set_icon_name: Some(
                                if model.dnd {
                                    "ld-bell-off-symbolic"
                                } else {
                                    "ld-bell-symbolic"
                                }
                            ),
                        },
                        #[template_child]
                        title {
                            set_label: &t!("notification-dropdown-empty-title"),
                        },
                        #[template_child]
                        description {
                            set_label: &t!("notification-dropdown-empty-description"),
                        },
                    },

                    gtk::ScrolledWindow {
                        add_css_class: "notification-dropdown-scroll",
                        set_vexpand: true,
                        set_hscrollbar_policy: gtk::PolicyType::Never,
                        #[watch]
                        set_visible: !model.showing_history && model.has_notifications,

                        #[local_ref]
                        groups_widget -> gtk::Box {
                            add_css_class: "notification-dropdown-groups",
                            set_orientation: gtk::Orientation::Vertical,
                        },
                    },

                    gtk::ScrolledWindow {
                        add_css_class: "notification-dropdown-scroll",
                        set_vexpand: true,
                        set_hscrollbar_policy: gtk::PolicyType::Never,
                        #[watch]
                        set_visible: model.showing_history,

                        #[local_ref]
                        history_widget -> gtk::Box {
                            add_css_class: "notification-dropdown-groups",
                            set_orientation: gtk::Orientation::Vertical,
                        },
                    },
                },
            },
        }
    }

    fn init(
        init: Self::Init,
        _root: Self::Root,
        sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        let scale = init.config.config().styling.scale.get().value();
        let dnd = init.notification.dnd.get();

        let groups = FactoryVecDeque::builder()
            .launch(gtk::Box::default())
            .forward(sender.input_sender(), |group_output| match group_output {
                NotificationGroupOutput::Dismissed => {
                    NotificationDropdownMsg::NotificationDismissed
                }
            });

        let history_items = FactoryVecDeque::builder()
            .launch(gtk::Box::default())
            .forward(sender.input_sender(), |item_output| match item_output {
                NotificationItemOutput::Dismissed(id) => {
                    NotificationDropdownMsg::HistoryItemDismissed(id)
                }
            });

        let mut model = Self {
            notification: init.notification.clone(),
            config: init.config.clone(),
            scaled_width: scaled_dimension(BASE_WIDTH, scale),
            scaled_height: scaled_dimension(BASE_HEIGHT, scale),
            dnd,
            has_notifications: false,
            showing_history: false,
            history: init.history.clone(),
            groups,
            history_items,
        };

        model.rebuild_groups();

        watchers::spawn(&sender, &init.notification, &init.config);
        watchers::spawn_history(&sender, &init.history);

        let groups_widget = model.groups.widget();
        let history_widget = model.history_items.widget();
        let widgets = view_output!();

        ComponentParts { model, widgets }
    }

    fn update(&mut self, msg: Self::Input, _sender: ComponentSender<Self>, _root: &Self::Root) {
        match msg {
            NotificationDropdownMsg::DndToggled(active) => {
                self.notification.set_dnd(active);
            }

            NotificationDropdownMsg::ClearAll => {
                let notifications = self.notification.notifications.get();
                for notification in &notifications {
                    notification.dismiss();
                }
            }

            NotificationDropdownMsg::NotificationDismissed => {}

            NotificationDropdownMsg::ToggleHistory => {
                self.showing_history = !self.showing_history;
                if self.showing_history {
                    self.rebuild_history_items();
                }
            }

            NotificationDropdownMsg::HistoryItemDismissed(id) => {
                let mut list = self.history.get();
                list.retain(|notif| notif.id != id);
                self.history.set(list);
            }
        }
    }

    fn update_cmd(
        &mut self,
        msg: Self::CommandOutput,
        _sender: ComponentSender<Self>,
        _root: &Self::Root,
    ) {
        match msg {
            NotificationDropdownCmd::NotificationsChanged => {
                self.rebuild_groups();
            }

            NotificationDropdownCmd::DndChanged(dnd) => {
                self.dnd = dnd;
            }

            NotificationDropdownCmd::ScaleChanged(scale) => {
                self.scaled_width = scaled_dimension(BASE_WIDTH, scale);
                self.scaled_height = scaled_dimension(BASE_HEIGHT, scale);
            }

            NotificationDropdownCmd::IconSourceChanged => {
                self.force_rebuild_groups();
            }

            NotificationDropdownCmd::TimeTick => {
                for idx in 0..self.groups.len() {
                    self.groups.send(idx, NotificationGroupInput::RefreshTime);
                }
            }

            NotificationDropdownCmd::HistoryChanged => {
                if self.showing_history {
                    self.rebuild_history_items();
                }
            }
        }
    }
}
