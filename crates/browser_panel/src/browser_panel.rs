use anyhow::Result;
use collections::VecDeque;
use db::kvp::KEY_VALUE_STORE;
use editor::Editor;
use gpui::{
    actions, div, prelude::*, Action, App, AsyncApp, AsyncWindowContext, Context, DismissEvent,
    Entity, EventEmitter, FocusHandle, Focusable, InteractiveElement, IntoElement, ParentElement,
    Pixels, Render, SharedString, StatefulInteractiveElement, Styled, WeakEntity, Window,
};
use serde::{Deserialize, Serialize};
use ui::{prelude::*, IconButton, IconButtonShape, IconName};
use ui_input::InputField;
use url::Url;
use util::ResultExt;
use workspace::{
    dock::{DockPosition, Panel, PanelEvent},
    Workspace,
};

const BROWSER_PANEL_KEY: &str = "BrowserPanel";
const DEFAULT_URL: &str = "https://zed.dev";
const MAX_HISTORY: usize = 100;

actions!(
    browser_panel,
    [
        /// Toggles the browser panel.
        Toggle,
        /// Toggles focus on the browser panel.
        ToggleFocus,
        /// Navigates back in browser history.
        GoBack,
        /// Navigates forward in browser history.
        GoForward,
        /// Reloads the current page.
        Reload,
        /// Stops loading the current page.
        Stop,
        /// Navigates to the URL in the address bar.
        Navigate,
        /// Focuses the address bar for URL input.
        FocusAddressBar,
    ]
);

#[derive(Clone, Debug, Serialize, Deserialize)]
struct BrowserHistoryEntry {
    url: String,
    title: Option<String>,
}

#[derive(Serialize, Deserialize)]
struct SerializedBrowserPanel {
    width: Option<f32>,
    height: Option<f32>,
    current_url: Option<String>,
    history: Vec<BrowserHistoryEntry>,
    history_index: Option<usize>,
}

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

enum BrowserLoadState {
    Idle,
    Loading,
    Loaded,
    Error(String),
}

pub struct BrowserPanel {
    workspace: WeakEntity<Workspace>,
    focus_handle: FocusHandle,
    address_bar: Entity<InputField>,
    width: Option<Pixels>,
    height: Option<Pixels>,

    // Browser state
    current_url: String,
    page_title: Option<String>,
    load_state: BrowserLoadState,

    // History management
    history: VecDeque<BrowserHistoryEntry>,
    history_index: Option<usize>,
}

impl BrowserPanel {
    pub fn new(workspace: &Workspace, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let address_bar = cx.new(|cx| InputField::new(window, cx, "Enter URL or search..."));

        let mut panel = Self {
            workspace: workspace.weak_handle(),
            focus_handle: cx.focus_handle(),
            address_bar: address_bar.clone(),
            width: None,
            height: None,
            current_url: DEFAULT_URL.to_string(),
            page_title: None,
            load_state: BrowserLoadState::Idle,
            history: VecDeque::new(),
            history_index: None,
        };

        // Set initial URL in address bar
        address_bar.update(cx, |input, cx| {
            input.editor.update(cx, |editor, window, cx| {
                editor.set_text(DEFAULT_URL, window, cx);
            });
        });

        // Initialize with default page
        panel.add_to_history(DEFAULT_URL.to_string(), Some("Zed - Code at the speed of thought".to_string()));
        panel
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

                    if let Some(url) = serialized.current_url {
                        browser_panel.current_url = url.clone();
                        browser_panel.address_bar_text = url;
                    }

                    // Restore history
                    if !serialized.history.is_empty() {
                        browser_panel.history = serialized.history.into_iter().collect();
                        browser_panel.history_index = serialized.history_index;
                    }
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
            current_url: Some(self.current_url.clone()),
            history: self.history.iter().cloned().collect(),
            history_index: self.history_index,
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

    fn add_to_history(&mut self, url: String, title: Option<String>) {
        // Remove any forward history when navigating to a new page
        if let Some(index) = self.history_index {
            self.history.truncate(index + 1);
        }

        // Add new entry
        self.history.push_back(BrowserHistoryEntry {
            url,
            title,
        });

        // Limit history size
        while self.history.len() > MAX_HISTORY {
            self.history.pop_front();
        }

        self.history_index = Some(self.history.len() - 1);
    }

    fn can_go_back(&self) -> bool {
        self.history_index.map_or(false, |index| index > 0)
    }

    fn can_go_forward(&self) -> bool {
        self.history_index.map_or(false, |index| index < self.history.len() - 1)
    }

    fn go_back(&mut self, cx: &mut Context<Self>) {
        if let Some(index) = self.history_index {
            if index > 0 {
                self.history_index = Some(index - 1);
                if let Some(entry) = self.history.get(index - 1) {
                    self.navigate_to_url(entry.url.clone(), cx);
                }
            }
        }
    }

    fn go_forward(&mut self, cx: &mut Context<Self>) {
        if let Some(index) = self.history_index {
            if index < self.history.len() - 1 {
                self.history_index = Some(index + 1);
                if let Some(entry) = self.history.get(index + 1) {
                    self.navigate_to_url(entry.url.clone(), cx);
                }
            }
        }
    }

    fn reload(&mut self, cx: &mut Context<Self>) {
        let url = self.current_url.clone();
        self.navigate_to_url(url, cx);
    }

    fn stop(&mut self, cx: &mut Context<Self>) {
        self.load_state = BrowserLoadState::Idle;
        cx.notify();
    }

    fn navigate_to_url(&mut self, url: String, cx: &mut Context<Self>) {
        // Validate and normalize URL
        let normalized_url = if url.starts_with("http://") || url.starts_with("https://") {
            url.clone()
        } else if url.contains('.') && !url.contains(' ') {
            format!("https://{}", url)
        } else {
            format!("https://www.google.com/search?q={}", urlencoding::encode(&url))
        };

        // Check if URL is valid
        if Url::parse(&normalized_url).is_err() {
            self.load_state = BrowserLoadState::Error("Invalid URL".to_string());
            cx.notify();
            return;
        }

        self.current_url = normalized_url.clone();

        // Update address bar
        let url_clone = normalized_url.clone();
        self.address_bar.update(cx, |input, cx| {
            input.editor.update(cx, |editor, window, cx| {
                editor.set_text(&url_clone, window, cx);
            });
        });

        self.load_state = BrowserLoadState::Loading;

        // Here we would integrate with actual browser engine (wry, servo, etc.)
        // For now, simulate loading
        cx.spawn_in(self.focus_handle(cx), async move |this, mut cx| {
            // Simulate network delay
            smol::Timer::after(std::time::Duration::from_millis(500)).await;

            this.update(&mut cx, |this, cx| {
                this.load_state = BrowserLoadState::Loaded;

                // Extract domain for title
                if let Ok(parsed_url) = Url::parse(&normalized_url) {
                    if let Some(domain) = parsed_url.host_str() {
                        this.page_title = Some(domain.to_string());
                    }
                }

                cx.notify();
            })
        }).detach();

        cx.notify();
    }

    fn navigate(&mut self, cx: &mut Context<Self>) {
        let url = self.address_bar.read(cx).editor.read(cx).text(cx);
        self.add_to_history(url.clone(), None);
        self.navigate_to_url(url, cx);
        self.serialize(cx);
    }

    fn focus_address_bar(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let focus_handle = self.address_bar.focus_handle(cx);
        window.focus(&focus_handle);
        cx.notify();
    }

    fn render_toolbar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let can_go_back = self.can_go_back();
        let can_go_forward = self.can_go_forward();
        let is_loading = matches!(self.load_state, BrowserLoadState::Loading);

        h_flex()
            .gap_1()
            .child(
                IconButton::new("back", IconName::ChevronLeft)
                    .shape(IconButtonShape::Square)
                    .disabled(!can_go_back)
                    .tooltip(|cx| Tooltip::text("Go Back", cx))
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.go_back(cx);
                    })),
            )
            .child(
                IconButton::new("forward", IconName::ChevronRight)
                    .shape(IconButtonShape::Square)
                    .disabled(!can_go_forward)
                    .tooltip(|cx| Tooltip::text("Go Forward", cx))
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.go_forward(cx);
                    })),
            )
            .child(
                if is_loading {
                    IconButton::new("stop", IconName::Close)
                        .shape(IconButtonShape::Square)
                        .tooltip(|cx| Tooltip::text("Stop", cx))
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.stop(cx);
                        }))
                } else {
                    IconButton::new("reload", IconName::ArrowCircle)
                        .shape(IconButtonShape::Square)
                        .tooltip(|cx| Tooltip::text("Reload", cx))
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.reload(cx);
                        }))
                },
            )
    }

    fn render_address_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        h_flex()
            .flex_1()
            .gap_2()
            .items_center()
            .px_1()
            .py_1()
            .child(Icon::new(IconName::Globe).size(IconSize::Small).color(Color::Muted))
            .child(div().flex_1().child(self.address_bar.clone()))
    }

    fn render_content(&self, cx: &mut Context<Self>) -> impl IntoElement {
        match &self.load_state {
            BrowserLoadState::Idle => {
                self.render_placeholder("Ready to browse", cx)
            }
            BrowserLoadState::Loading => {
                self.render_placeholder("Loading...", cx)
            }
            BrowserLoadState::Loaded => {
                self.render_web_view(cx)
            }
            BrowserLoadState::Error(error) => {
                self.render_error(error, cx)
            }
        }
    }

    fn render_placeholder(&self, message: &str, cx: &mut Context<Self>) -> impl IntoElement {
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
                        Label::new(message)
                            .size(LabelSize::Large)
                            .color(Color::Muted),
                    )
                    .child(
                        Label::new(format!("URL: {}", self.current_url))
                            .size(LabelSize::Small)
                            .color(Color::Disabled),
                    ),
            )
    }

    fn render_web_view(&self, cx: &mut Context<Self>) -> impl IntoElement {
        // This is where the actual WebView rendering would be integrated
        // For example, with wry or servo
        div()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .size_full()
            .bg(cx.theme().colors().editor_background)
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_4()
                    .items_center()
                    .p_8()
                    .child(Icon::new(IconName::Globe).size(IconSize::XLarge))
                    .child(
                        Label::new("Browser View")
                            .size(LabelSize::Large)
                            .color(Color::Default),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_2()
                            .items_start()
                            .child(
                                Label::new(format!("Current URL: {}", self.current_url))
                                    .size(LabelSize::Small)
                                    .color(Color::Muted),
                            )
                            .when_some(self.page_title.as_ref(), |this, title| {
                                this.child(
                                    Label::new(format!("Title: {}", title))
                                        .size(LabelSize::Small)
                                        .color(Color::Muted),
                                )
                            })
                            .child(
                                Label::new(format!("History: {} pages", self.history.len()))
                                    .size(LabelSize::Small)
                                    .color(Color::Muted),
                            ),
                    )
                    .child(
                        div()
                            .mt_4()
                            .p_4()
                            .bg(cx.theme().colors().surface_background)
                            .rounded_md()
                            .child(
                                Label::new("WebView integration ready for:")
                                    .size(LabelSize::Small)
                                    .color(Color::Muted),
                            )
                            .child(
                                Label::new("• Servo browser engine")
                                    .size(LabelSize::Small)
                                    .color(Color::Disabled),
                            )
                            .child(
                                Label::new("• Platform native WebView (wry)")
                                    .size(LabelSize::Small)
                                    .color(Color::Disabled),
                            )
                            .child(
                                Label::new("• Custom rendering backend")
                                    .size(LabelSize::Small)
                                    .color(Color::Disabled),
                            ),
                    ),
            )
    }

    fn render_error(&self, error: &str, cx: &mut Context<Self>) -> impl IntoElement {
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
                    .child(Icon::new(IconName::XCircle).size(IconSize::XLarge).color(Color::Error))
                    .child(
                        Label::new("Error Loading Page")
                            .size(LabelSize::Large)
                            .color(Color::Error),
                    )
                    .child(
                        Label::new(error)
                            .size(LabelSize::Small)
                            .color(Color::Muted),
                    ),
            )
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
                // Top toolbar with navigation controls
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .px_2()
                    .py_1()
                    .border_b_1()
                    .border_color(cx.theme().colors().border)
                    .child(self.render_toolbar(cx))
                    .child(self.render_address_bar(cx)),
            )
            .child(
                // Browser content area
                div()
                    .flex_1()
                    .size_full()
                    .child(self.render_content(cx)),
            )
            .on_action(cx.listener(BrowserPanel::go_back))
            .on_action(cx.listener(BrowserPanel::go_forward))
            .on_action(cx.listener(BrowserPanel::reload))
            .on_action(cx.listener(BrowserPanel::stop))
            .on_action(cx.listener(BrowserPanel::navigate))
            .on_action(cx.listener(BrowserPanel::focus_address_bar))
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
                self.width.unwrap_or(Pixels::from(600.0))
            }
            DockPosition::Bottom => self.height.unwrap_or(Pixels::from(400.0)),
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
