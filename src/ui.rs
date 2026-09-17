use std::collections::HashMap;

use gtk::prelude::*;
use gtk4_layer_shell::{Edge, Layer, LayerShell};
use niri_ipc::{Action, Request};
use relm4::factory::{DynamicIndex, FactoryComponent, FactoryVecDeque};
use relm4::prelude::*;

use crate::niri::CommandClient;

/// turn a gtk::ApplicationWindow into a layer shell surface
pub fn init_layer_surface(window: &gtk::ApplicationWindow, height: i32) {
    window.init_layer_shell();
    window.set_layer(Layer::Top);
    window.set_anchor(Edge::Bottom, true);
    window.set_anchor(Edge::Left, true);
    window.set_anchor(Edge::Right, true);
    window.set_exclusive_zone(height);
    window.set_default_size(-1, height);
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
            set_css_classes: if self.focused { &["dock-icon", "fcused"] } else { &["dock-icon"] },
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

pub struct DockModel {
    icons: FactoryVecDeque<IconModel>,
    /// this will be need to be kept in lockstep with icons
    index_of: HashMap<u64, DynamicIndex>,
    focused_id: Option<u64>,
    commands: CommandClient,
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
}

#[relm4::component(pub)]
impl SimpleComponent for DockModel {
    type Init = CommandClient;
    type Input = DockMsg;
    type Output = ();

    view! {
        #[root]
        gtk::ApplicationWindow {
            set_title: Some("dock"),

            #[wrap(Some)]
            set_child = &gtk::Box {
                set_orientation: gtk::Orientation::Horizontal,
                set_spacing: 4,
                #[local_ref]
                icon_box -> gtk::Box {}
            }
        }
    }

    fn init(
        commands: Self::Init,
        root: Self::Root,
        sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        init_layer_surface(&root, 48);

        let icons = FactoryVecDeque::builder()
            .launch(gtk::Box::default())
            .forward(sender.input_sender(), |output| match output {
                IconOutput::Clicked(id) => DockMsg::IconClicked(id),
            });

        crate::start_niri_events(sender.input_sender().clone());

        let model = DockModel {
            icons,
            index_of: HashMap::new(),
            focused_id: None,
            commands,
        };
        let icon_box = model.icons.widget();
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
            }
            DockMsg::WindowClosed { id } => {
                if let Some(index) = self.index_of.remove(&id) {
                    guard.remove(index.current_index());
                }
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
        }
    }
}
