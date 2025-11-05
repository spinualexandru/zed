use anyhow::Result;
use db::kvp::KEY_VALUE_STORE;
use gpui::{
    actions, prelude::*, Action, App, AsyncApp, AsyncWindowContext, Context, DismissEvent,
    Entity, EventEmitter, FocusHandle, Focusable, Pixels, Render, WeakEntity, Window,
};
use serde::{Deserialize, Serialize};
use ui::{prelude::*, IconName};
use util::ResultExt;
use workspace::{
    dock::{DockPosition, Panel, PanelEvent},
    Workspace,
};

const BROWSER_PANEL_KEY: &str = "BrowserPanel";

actions!(
    browser_panel,
    [
        /// Toggles the browser panel.
        Toggle,
        /// Toggles focus on the browser panel.
        ToggleFocus
    ]
);

pub fn init(cx: &mut App) {
    cx.observe_new(
        |workspace: &mut Workspace, _window, _: &mut Context<Workspace>| {
            workspace.register_action(|workspace, _: &ToggleFocus, window, cx| {
                workspace.toggle_panel_focus::<BrowserPanel>(window, cx);
            });
            workspace.register_action(|workspace, _: &Toggle, window, cx| {
                if !workspace.toggle_panel_focus::<BrowserPanel>(window, cx) {
                    workspace.close_panel::<BrowserPanel>(window, cx);
                }
            });
        },
    )
    .detach();
}

#[derive(Serialize, Deserialize)]
struct SerializedBrowserPanel {
    width: Option<f32>,
    height: Option<f32>,
}

pub struct BrowserPanel {
    workspace: WeakEntity<Workspace>,
    focus_handle: FocusHandle,
    width: Option<Pixels>,
    height: Option<Pixels>,
}

impl BrowserPanel {
    pub fn new(workspace: &Workspace, _window: &mut Window, cx: &mut Context<Self>) -> Self {
        Self {
            workspace: workspace.weak_handle(),
            focus_handle: cx.focus_handle(),
            width: None,
            height: None,
        }
    }

    pub async fn load(
        workspace: WeakEntity<Workspace>,
        mut cx: AsyncWindowContext,
    ) -> Result<Entity<Self>> {
        let mut serialized_panel = None;

        if let Some(serialization_key) = workspace
            .read_with(&cx, |workspace, _| {
                BrowserPanel::serialization_key(workspace)
            })
            .ok()
            .flatten()
        {
            serialized_panel = cx
                .background_spawn(async move { KEY_VALUE_STORE.read_kvp(&serialization_key) })
                .await
                .log_err()
                .flatten()
                .and_then(|panel| serde_json::from_str::<SerializedBrowserPanel>(&panel).ok());
        }

        workspace.update_in(&mut cx, |workspace, window, cx| {
            let panel = cx.new(|cx| {
                let mut browser_panel = BrowserPanel::new(workspace, window, cx);
                if let Some(serialized) = serialized_panel {
                    browser_panel.width = serialized.width.map(|w| Pixels::from(w));
                    browser_panel.height = serialized.height.map(|h| Pixels::from(h));
                }
                browser_panel
            });
            Ok(panel)
        })?
    }

    fn serialization_key(workspace: &Workspace) -> Option<String> {
        workspace
            .database_id()
            .map(|database_id| format!("{BROWSER_PANEL_KEY}_{database_id}"))
    }

    fn serialize(&mut self, cx: &mut Context<Self>) {
        let serialized = SerializedBrowserPanel {
            width: self.width.map(|w| w.0),
            height: self.height.map(|h| h.0),
        };

        if let Some(serialization_key) = self
            .workspace
            .read_with(cx, |workspace, _| BrowserPanel::serialization_key(workspace))
            .ok()
            .flatten()
        {
            cx.background_spawn(async move {
                KEY_VALUE_STORE
                    .write_kvp(
                        serialization_key,
                        serde_json::to_string(&serialized).unwrap(),
                    )
                    .await
                    .log_err();
            })
            .detach();
        }
    }
}

impl Render for BrowserPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .track_focus(&self.focus_handle)
            .flex()
            .flex_col()
            .size_full()
            .bg(cx.theme().colors().panel_background)
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_center()
                    .size_full()
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_2()
                            .items_center()
                            .child(Icon::new(IconName::Globe).size(IconSize::XLarge))
                            .child(
                                Label::new("Browser Panel")
                                    .size(LabelSize::Large)
                                    .color(Color::Muted),
                            )
                            .child(
                                Label::new("Servo browser integration coming soon")
                                    .size(LabelSize::Small)
                                    .color(Color::Disabled),
                            ),
                    ),
            )
    }
}

impl EventEmitter<PanelEvent> for BrowserPanel {}

impl Focusable for BrowserPanel {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Panel for BrowserPanel {
    fn persistent_name() -> &'static str {
        "BrowserPanel"
    }

    fn panel_key() -> &'static str {
        BROWSER_PANEL_KEY
    }

    fn position(&self, _window: &Window, _cx: &App) -> DockPosition {
        DockPosition::Right
    }

    fn position_is_valid(&self, _: DockPosition) -> bool {
        true
    }

    fn set_position(
        &mut self,
        _position: DockPosition,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) {
        // Position can be changed by user
    }

    fn size(&self, window: &Window, _cx: &App) -> Pixels {
        match self.position(window, _cx) {
            DockPosition::Left | DockPosition::Right => {
                self.width.unwrap_or(Pixels::from(400.0))
            }
            DockPosition::Bottom => self.height.unwrap_or(Pixels::from(300.0)),
        }
    }

    fn set_size(&mut self, size: Option<Pixels>, window: &mut Window, cx: &mut Context<Self>) {
        match self.position(window, cx) {
            DockPosition::Left | DockPosition::Right => self.width = size,
            DockPosition::Bottom => self.height = size,
        }
        cx.notify();
        cx.defer_in(window, |this, _, cx| {
            this.serialize(cx);
        })
    }

    fn icon(&self, _window: &Window, _cx: &App) -> Option<IconName> {
        Some(IconName::Globe)
    }

    fn icon_tooltip(&self, _window: &Window, _cx: &App) -> Option<&'static str> {
        Some("Browser Panel")
    }

    fn toggle_action(&self) -> Box<dyn Action> {
        Box::new(ToggleFocus)
    }

    fn activation_priority(&self) -> u32 {
        5
    }
}

impl EventEmitter<DismissEvent> for BrowserPanel {}
