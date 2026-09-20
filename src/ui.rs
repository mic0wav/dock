use std::collections::HashMap;

use gtk::prelude::*;
use gtk4_layer_shell::{Edge, Layer, LayerShell};
use niri_ipc::{Action, Request, Window};
use relm4::factory::{DynamicIndex, FactoryComponent, FactoryVecDeque};
use relm4::prelude::*;

use crate::config::{self, Position};
use crate::niri::CommandClient;

/// make sure we do not include the dock in the dock
pub const APP_ID: &str = "dev.example.niri-dock";

pub(crate) fn is_own_window(w: &Window) -> bool {
    w.app_id.as_deref() == Some(APP_ID)
}

/// puts window in layer shell mode and anchored to a edge
fn apply_layer_position(window: &impl LayerShell, position: Position) {
    window.set_layer(Layer::Top);
    for (edge, active) in [
        (Edge::Left, false),
        (Edge::Right, false),
        (Edge::Top, position == Position::Top),
        (Edge::Bottom, position == Position::Bottom),
    ] {
        window.set_anchor(edge, active);
    }
}

pub struct IconInit {
    pub id: u64,
    pub app_id: Option<String>,
    pub title: String,
}

#[derive(Debug)]
pub struct IconModel {
    pub id: u64,
    pub app_id: Option<String>,
    pub title: String,
    pub focused: bool,
}

#[derive(Debug)]
pub enum IconMsg {
    SetFocused(bool),
    Retitle(String),
}

#[derive(Debug)]
pub enum IconOutput {
    Clicked(u64),
}

#[relm4::factory(pub)]
impl FactoryComponent for IconModel {
    type Init = IconInit;
    type Input = IconMsg;
    type Output = IconOutput;
    type CommandOutput = ();
    type ParentWidget = gtk::Box;

    view! {
        #[root]
        gtk::Button {
            #[watch]
            set_css_classes: if self.focused { &["app", "active"] } else { &["app"] },
            set_tooltip_text: Some(&self.title),

            connect_clicked[sender, id = self.id] => move |_| {
                let _ = sender.output(IconOutput::Clicked(id));
            },

            gtk::Image {
                // plug in icon cache once implemented
                set_icon_name: Some (
                    self.app_id.as_deref().unwrap_or("application-x-executable")
                ),
                set_pixel_size: 32,
            },
        }
    }

    fn init_model(init: Self::Init, _index: &DynamicIndex, _sender: FactorySender<Self>) -> Self {
        Self {
            id: init.id,
            app_id: init.app_id,
            title: init.title,
            focused: false,
        }
    }

    fn update(&mut self, msg: Self::Input, _sender: FactorySender<Self>) {
        match msg {
            IconMsg::SetFocused(focused) => self.focused = focused,
            IconMsg::Retitle(title) => self.title = title,
        }
    }
}

pub struct PinInit {
    pub icon: String,
    pub command: String,
}

#[derive(Debug)]
pub struct PinModel {
    icon: String,
    command: String,
}

#[derive(Debug)]
pub enum PinOutput {
    Clicked(String),
}

#[relm4::factory(pub)]
impl FactoryComponent for PinModel {
    type Init = PinInit;
    type Input = ();
    type Output = PinOutput;
    type CommandOutput = ();
    type ParentWidget = gtk::Box;

    view! {
        #[root]
        gtk::Button {
            set_css_classes: &["app"],

            connect_clicked[sender, command = self.command.clone()] => move |_| {
                let _ = sender.output(PinOutput::Clicked(command.clone()));
            },

            gtk::Image {
                set_icon_name: Some(&self.icon),
                set_pixel_size: 32
            }
        }
    }

    fn init_model(init: Self::Init, _index: &DynamicIndex, _sender: FactorySender<Self>) -> Self {
        Self {
            icon: init.icon,
            command: init.command,
        }
    }
}

pub struct IndicatorModel {
    visible: bool,
    window: gtk::Window,
}

#[derive(Debug)]
pub enum IndicatorMsg {
    Entered,
    Reappear,
    SetPosition(Position),
}

#[derive(Debug)]
pub enum IndicatorOutput {
    Entered,
}

#[relm4::component(pub)]
impl SimpleComponent for IndicatorModel {
    type Init = Position;
    type Input = IndicatorMsg;
    type Output = IndicatorOutput;

    view! {
        #[root]
        gtk::Window {
            #[watch]
            set_visible: model.visible,

            gtk::Box {
                set_css_classes: &["indicator"],
                set_margin_all: 2,
                add_controller = gtk::EventControllerMotion {
                    connect_enter[sender] => move |_, _, _| {
                        sender.input(IndicatorMsg::Entered);
                    }
                },
            }
        }
    }

    fn init(
        position: Self::Init,
        root: Self::Root,
        sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        root.init_layer_shell();
        apply_layer_position(&root, position);

        let model = IndicatorModel {
            visible: true,
            window: root.clone(),
        };
        let widgets = view_output!();
        ComponentParts { model, widgets }
    }

    fn update(&mut self, msg: Self::Input, sender: ComponentSender<Self>) {
        match msg {
            IndicatorMsg::Entered => {
                self.visible = false;
                let _ = sender.output(IndicatorOutput::Entered);
            }
            IndicatorMsg::Reappear => {
                self.visible = true;
            }
            IndicatorMsg::SetPosition(position) => {
                apply_layer_position(&self.window, position);
            }
        }
    }
}

pub struct DockModel {
    icons: FactoryVecDeque<IconModel>,
    /// this will be need to be kept in lockstep with icons
    index_of: HashMap<u64, DynamicIndex>,
    focused_id: Option<u64>,
    windows_count: usize,
    commands: CommandClient,
    window: gtk::ApplicationWindow,
    css_provider: gtk::CssProvider,
    pinned: FactoryVecDeque<PinModel>,
    visible: bool,
    indicator: Controller<IndicatorModel>,
}

pub struct DockInit {
    pub commands: CommandClient,
    pub config: config::Config,
}

#[derive(Debug)]
pub enum DockMsg {
    WindowUpserted {
        id: u64,
        app_id: Option<String>,
        title: String,
    },
    WindowClosed {
        id: u64,
    },
    WindowFocusedChanged {
        id: Option<u64>,
    },
    IconClicked(u64),
    PinClicked(String),
    ConfigReloaded(config::Config),
    IndicatorEntered,
    PointerLeft,
}

#[relm4::component(pub)]
impl SimpleComponent for DockModel {
    type Init = DockInit;
    type Input = DockMsg;
    type Output = ();

    view! {
        #[root]
        gtk::ApplicationWindow {
            set_title: Some("dock"),
            #[watch]
            set_visible: model.visible,

            #[wrap(Some)]
            set_child = &gtk::Box {
                set_orientation: gtk::Orientation::Horizontal,
                set_spacing: 8,
                add_controller = gtk::EventControllerMotion {
                    connect_leave[sender] => move |_| {
                        sender.input(DockMsg::PointerLeft);
                    }
                },

                #[local_ref]
                pinned_box -> gtk::Box {
                    set_css_classes: &["dock"],
                    set_spacing: 8,
                },
                #[local_ref]
                icon_box -> gtk::Box {
                    #[watch]
                    set_visible: model.windows_count > 0,
                    set_css_classes: &["dock"],
                    set_spacing: 8,
                }
            }
        }
    }

    fn init(
        init: Self::Init,
        root: Self::Root,
        sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        let DockInit { commands, config } = init;
        root.init_layer_shell();
        apply_layer_position(&root, config.position);

        let css_provider = gtk::CssProvider::new();
        css_provider.load_from_string(&config::load_css());
        if let Some(display) = gtk::gdk::Display::default() {
            gtk::style_context_add_provider_for_display(
                &display,
                &css_provider,
                gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
            );
        }

        if config.hot_reload {
            let sender_for_reload = sender.input_sender().clone();
            config::watch_changes(move || {
                let _ = sender_for_reload.send(DockMsg::ConfigReloaded(config::load()));
            });
        }

        let icons = FactoryVecDeque::builder()
            .launch(gtk::Box::default())
            .forward(sender.input_sender(), |output| match output {
                IconOutput::Clicked(id) => DockMsg::IconClicked(id),
            });

        let mut pinned = FactoryVecDeque::builder()
            .launch(gtk::Box::default())
            .forward(sender.input_sender(), |output| match output {
                PinOutput::Clicked(command) => DockMsg::PinClicked(command),
            });
        {
            let mut guard = pinned.guard();
            for entry in config.pinned.values() {
                guard.push_back(PinInit {
                    icon: entry.icon.clone(),
                    command: entry.command.clone(),
                });
            }
        }

        crate::start_niri_events(sender.input_sender().clone());

        // seperate from main window, has to be registered seperately too
        let indicator = IndicatorModel::builder().launch(config.position).forward(
            sender.input_sender(),
            |output| match output {
                IndicatorOutput::Entered => DockMsg::IndicatorEntered,
            },
        );
        relm4::main_application().add_window(indicator.widget());

        let window = root.clone();
        let model = DockModel {
            icons,
            index_of: HashMap::new(),
            focused_id: None,
            windows_count: 0,
            commands,
            window,
            css_provider,
            pinned,
            visible: false,
            indicator,
        };
        let icon_box = model.icons.widget();
        let pinned_box = model.pinned.widget();
        let widgets = view_output!();
        ComponentParts { model, widgets }
    }

    fn update(&mut self, msg: Self::Input, _sender: ComponentSender<Self>) {
        let mut guard = self.icons.guard();
        match msg {
            DockMsg::WindowUpserted { id, app_id, title } => {
                if let Some(index) = self.index_of.get(&id) {
                    guard.send(index.current_index(), IconMsg::Retitle(title));
                } else {
                    let index = guard.push_back(IconInit { id, app_id, title });
                    self.index_of.insert(id, index);
                }
                self.windows_count = self.index_of.len();
            }
            DockMsg::WindowClosed { id } => {
                if let Some(index) = self.index_of.remove(&id) {
                    guard.remove(index.current_index());
                }
                self.windows_count = self.index_of.len();
            }
            DockMsg::WindowFocusedChanged { id } => {
                if let Some(old_id) = self.focused_id
                    && let Some(old_index) = self.index_of.get(&old_id)
                {
                    guard.send(old_index.current_index(), IconMsg::SetFocused(false));
                }
                if let Some(new_id) = id
                    && let Some(index) = self.index_of.get(&new_id)
                {
                    guard.send(index.current_index(), IconMsg::SetFocused(true));
                }
                self.focused_id = id;
            }
            DockMsg::IconClicked(id) => {
                let commands = self.commands.clone();
                crate::runtime().spawn(async move {
                    if let Err(e) = commands
                        .send(Request::Action(Action::FocusWindow { id }))
                        .await
                    {
                        log::warn!("focus window failed for {id}: {e:#}");
                    }
                });
            }
            DockMsg::PinClicked(command) => {
                let commands = self.commands.clone();
                crate::runtime().spawn(async move {
                    let result = commands
                        .send(Request::Action(Action::SpawnSh {
                            command: command.clone(),
                        }))
                        .await;
                    if let Err(e) = result {
                        log::warn!("spawn failed for '{command}': {e:#}");
                    }
                });
            }
            DockMsg::ConfigReloaded(config) => {
                apply_layer_position(&self.window, config.position);
                self.indicator
                    .emit(IndicatorMsg::SetPosition(config.position));
                self.css_provider.load_from_string(&config::load_css());

                let mut guard = self.pinned.guard();
                guard.clear();
                for entry in config.pinned.values() {
                    guard.push_back(PinInit {
                        icon: entry.icon.clone(),
                        command: entry.command.clone(),
                    });
                }

                log::info!(
                    "config reloaded: position={:?} hot_reload={} pinned={}",
                    config.position,
                    config.hot_reload,
                    config.pinned.len(),
                );
            }
            DockMsg::IndicatorEntered => {
                self.visible = true;
            }
            DockMsg::PointerLeft => {
                self.visible = false;
                self.indicator.emit(IndicatorMsg::Reappear);
            }
        }
    }
}
