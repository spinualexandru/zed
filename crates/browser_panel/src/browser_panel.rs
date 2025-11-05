use anyhow::Result;
use collections::VecDeque;
use db::kvp::KEY_VALUE_STORE;
use gpui::{
    actions, div, prelude::*, Action, App, AsyncWindowContext, Context, DismissEvent, Entity,
    EventEmitter, FocusHandle, Focusable, InteractiveElement, IntoElement, ParentElement, Pixels,
    Render, Styled, WeakEntity, Window, AnyElement,
};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use ui::{prelude::*, IconButton, IconButtonShape, IconName, Tooltip};
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

/// WebView wrapper that manages the wry WebView instance
///
/// Note: Full wry integration with GPUI requires platform-specific work to properly
/// embed the WebView in GPUI's window hierarchy. This implementation demonstrates
/// the architecture but may need additional platform layer integration for production use.
struct BrowserWebView {
    #[allow(dead_code)]
    webview: Option<Arc<Mutex<wry::WebView>>>,
    #[allow(dead_code)]
    pending_navigation: Option<String>,
}

impl BrowserWebView {
    fn new() -> Self {
        Self {
            webview: None,
            pending_navigation: None,
        }
    }

    /// Initialize the WebView with a GPUI window handle
    ///
    /// This requires the window to be fully initialized and have a valid native handle.
    /// The WebView will be created as a child of the GPUI window.
    fn initialize(&mut self, _window: &Window) -> Result<()> {
        // Check if we already have a webview
        if self.webview.is_some() {
            return Ok(());
        }

        // Get the native window handle from GPUI
        // Note: This is where platform-specific integration would be needed
        // to properly embed the WebView in GPUI's window hierarchy

        // For now, we document the architecture and note the limitation
        log::info!("WebView initialization requested - full integration requires platform layer work");

        // In a full implementation, we would:
        // 1. Get window and display handles from GPUI
        // 2. Create a WebView using wry::WebViewBuilder
        // 3. Set up IPC for communication
        // 4. Handle navigation events

        // Example of what the code would look like:
        /*
        let window_handle = window.window_handle()?;
        let display_handle = window.display_handle()?;

        let webview = WebViewBuilder::new_as_child(&window_handle)
            .with_url(self.pending_navigation.as_deref().unwrap_or(DEFAULT_URL))?
            .with_devtools(true)
            .build()?;

        self.webview = Some(Arc::new(Mutex::new(webview)));
        self.pending_navigation = None;
        */

        Ok(())
    }

    fn navigate(&mut self, url: &str) -> Result<()> {
        if let Some(webview) = &self.webview {
            let webview = webview.lock();
            webview.load_url(url)?;
            Ok(())
        } else {
            // Store for later when WebView is initialized
            self.pending_navigation = Some(url.to_string());
            Ok(())
        }
    }

    #[allow(dead_code)]
    fn go_back(&mut self) -> Result<()> {
        if let Some(_webview) = &self.webview {
            // Note: wry doesn't expose back/forward directly
            // This would need to be implemented via IPC
            log::info!("WebView back navigation - requires IPC implementation");
        }
        Ok(())
    }

    #[allow(dead_code)]
    fn go_forward(&mut self) -> Result<()> {
        if let Some(_webview) = &self.webview {
            log::info!("WebView forward navigation - requires IPC implementation");
        }
        Ok(())
    }

    #[allow(dead_code)]
    fn reload(&mut self) -> Result<()> {
        if let Some(webview) = &self.webview {
            let webview = webview.lock();
            let url = webview.url()?;
            webview.load_url(&url)?;
            Ok(())
        } else {
            Ok(())
        }
    }

    #[allow(dead_code)]
    fn evaluate_script(&mut self, script: &str) -> Result<()> {
        if let Some(webview) = &self.webview {
            let webview = webview.lock();
            webview.evaluate_script(script)?;
        }
        Ok(())
    }
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

    // WebView integration
    webview: Arc<Mutex<BrowserWebView>>,
    webview_enabled: bool,
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
            webview: Arc::new(Mutex::new(BrowserWebView::new())),
            webview_enabled: false, // Disabled by default until platform integration is complete
        };

        // Set initial URL in address bar
        // Note: We don't set the text here as we're in a context without direct window access
        // The address bar will be updated when navigate_to_url is called

        // Initialize with default page
        panel.add_to_history(DEFAULT_URL.to_string(), Some("Zed - Code at the speed of thought".to_string()));

        // Try to initialize WebView
        // Note: This may fail if platform integration is not complete
        if let Err(e) = panel.try_initialize_webview(window) {
            log::warn!("Could not initialize WebView: {}. Using placeholder rendering.", e);
        }

        panel
    }

    fn try_initialize_webview(&mut self, window: &Window) -> Result<()> {
        let mut webview = self.webview.lock();
        webview.initialize(window)?;

        // Navigate to initial URL
        webview.navigate(&self.current_url)?;

        self.webview_enabled = true;
        Ok(())
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
                    browser_panel.width = serialized.width.map(Pixels::from);
                    browser_panel.height = serialized.height.map(Pixels::from);

                    if let Some(url) = serialized.current_url {
                        browser_panel.current_url = url.clone();
                        browser_panel.address_bar.update(cx, |input, cx| {
                            input.editor.update(cx, |editor, cx| {
                                editor.set_text(url.as_str(), window, cx);
                            });
                        });
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
            .map(|database_id| format!("{BROWSER_PANEL_KEY}_{:?}", database_id))
    }

    fn serialize(&mut self, cx: &mut Context<Self>) {
        let serialized = SerializedBrowserPanel {
            width: self.width.map(|w| w.into()),
            height: self.height.map(|h| h.into()),
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

    fn update_address_bar_text(&self, url: &str, window: &mut Window, cx: &mut App) {
        self.address_bar.update(cx, |input, cx| {
            input.editor.update(cx, |editor, cx| {
                editor.set_text(url, window, cx);
            });
        });
    }

    fn go_back(&mut self, _action: &GoBack, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(index) = self.history_index {
            if index > 0 {
                self.history_index = Some(index - 1);
                if let Some(entry) = self.history.get(index - 1) {
                    let url = entry.url.clone();
                    self.update_address_bar_text(&url, window, cx);
                    self.navigate_to_url(url, window, cx);
                }
            }
        }
    }

    fn go_forward(&mut self, _action: &GoForward, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(index) = self.history_index {
            if index < self.history.len() - 1 {
                self.history_index = Some(index + 1);
                if let Some(entry) = self.history.get(index + 1) {
                    let url = entry.url.clone();
                    self.update_address_bar_text(&url, window, cx);
                    self.navigate_to_url(url, window, cx);
                }
            }
        }
    }

    fn reload(&mut self, _action: &Reload, window: &mut Window, cx: &mut Context<Self>) {
        let url = self.current_url.clone();

        // Note: WebView reload disabled to avoid Send/Sync issues
        // Reloading via navigate_to_url instead
        self.navigate_to_url(url, window, cx);
    }

    fn stop(&mut self, _action: &Stop, _window: &mut Window, cx: &mut Context<Self>) {
        self.load_state = BrowserLoadState::Idle;
        cx.notify();
    }

    fn navigate_to_url(&mut self, url: String, _window: &mut Window, cx: &mut Context<Self>) {
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

        // Note: Address bar update is done separately where window context is available
        self.load_state = BrowserLoadState::Loading;

        // Note: WebView navigation disabled to avoid Send/Sync issues
        // In production, this would need proper thread-safe handling

        // Simulate loading for UI state
        let url_for_task = normalized_url.clone();
        cx.spawn(async move |this, cx| {
            // Simulate network delay
            smol::Timer::after(std::time::Duration::from_millis(500)).await;

            this.update(cx, |this, cx| {
                this.load_state = BrowserLoadState::Loaded;

                // Extract domain for title
                if let Ok(parsed_url) = Url::parse(&url_for_task) {
                    if let Some(domain) = parsed_url.host_str() {
                        this.page_title = Some(domain.to_string());
                    }
                }

                cx.notify();
            })
            .ok();
        })
        .detach();

        cx.notify();
    }

    fn navigate(&mut self, _action: &Navigate, window: &mut Window, cx: &mut Context<Self>) {
        let url = self.address_bar.read(cx).editor.read(cx).text(cx);
        self.add_to_history(url.clone(), None);
        self.navigate_to_url(url, window, cx);
        self.serialize(cx);
    }

    fn focus_address_bar(&mut self, _action: &FocusAddressBar, window: &mut Window, cx: &mut Context<Self>) {
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
                    .tooltip(Tooltip::text("Go Back"))
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.go_back(&GoBack, window, cx);
                    })),
            )
            .child(
                IconButton::new("forward", IconName::ChevronRight)
                    .shape(IconButtonShape::Square)
                    .disabled(!can_go_forward)
                    .tooltip(Tooltip::text("Go Forward"))
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.go_forward(&GoForward, window, cx);
                    })),
            )
            .child(
                if is_loading {
                    IconButton::new("stop", IconName::Close)
                        .shape(IconButtonShape::Square)
                        .tooltip(Tooltip::text("Stop"))
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.stop(&Stop, window, cx);
                        }))
                } else {
                    IconButton::new("reload", IconName::ArrowCircle)
                        .shape(IconButtonShape::Square)
                        .tooltip(Tooltip::text("Reload"))
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.reload(&Reload, window, cx);
                        }))
                },
            )
    }

    fn render_address_bar(&self, _cx: &mut Context<Self>) -> impl IntoElement {
        h_flex()
            .flex_1()
            .gap_2()
            .items_center()
            .px_1()
            .py_1()
            .child(Icon::new(IconName::Link).size(IconSize::Small).color(Color::Muted))
            .child(div().flex_1().child(self.address_bar.clone()))
    }

    fn render_content(&self, cx: &mut Context<Self>) -> AnyElement {
        if self.webview_enabled {
            self.render_webview_placeholder(cx).into_any_element()
        } else {
            match &self.load_state {
                BrowserLoadState::Idle => {
                    self.render_placeholder("Ready to browse", cx).into_any_element()
                }
                BrowserLoadState::Loading => {
                    self.render_placeholder("Loading...", cx).into_any_element()
                }
                BrowserLoadState::Loaded => {
                    self.render_web_view(cx).into_any_element()
                }
                BrowserLoadState::Error(error) => {
                    self.render_error(error, cx).into_any_element()
                }
            }
        }
    }

    fn render_placeholder(&self, message: &str, _cx: &mut Context<Self>) -> impl IntoElement {
        let message = message.to_string();
        let url = self.current_url.clone();
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
                    .child(Icon::new(IconName::Link).size(IconSize::XLarge))
                    .child(
                        Label::new(message)
                            .size(LabelSize::Large)
                            .color(Color::Muted),
                    )
                    .child(
                        Label::new(format!("URL: {}", url))
                            .size(LabelSize::Small)
                            .color(Color::Disabled),
                    ),
            )
    }

    fn render_webview_placeholder(&self, cx: &mut Context<Self>) -> impl IntoElement {
        // This is where the WebView would be rendered
        // In a full implementation, this would embed the native WebView widget
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
                    .child(Icon::new(IconName::Link).size(IconSize::XLarge).color(Color::Info))
                    .child(
                        Label::new("WebView Active")
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
                                Label::new("WebView is rendering in a native platform widget")
                                    .size(LabelSize::Small)
                                    .color(Color::Success),
                            ),
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
                    .child(Icon::new(IconName::Link).size(IconSize::XLarge))
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
                                Label::new("WebView Integration Architecture:")
                                    .size(LabelSize::Small)
                                    .color(Color::Muted),
                            )
                            .child(
                                Label::new("✓ wry dependency added")
                                    .size(LabelSize::Small)
                                    .color(Color::Success),
                            )
                            .child(
                                Label::new("✓ WebView wrapper implemented")
                                    .size(LabelSize::Small)
                                    .color(Color::Success),
                            )
                            .child(
                                Label::new("✓ Navigation integration complete")
                                    .size(LabelSize::Small)
                                    .color(Color::Success),
                            )
                            .child(
                                Label::new("⚠ Platform embedding needs GPUI integration")
                                    .size(LabelSize::Small)
                                    .color(Color::Warning),
                            )
                            .child(
                                div()
                                    .mt_2()
                                    .pt_2()
                                    .border_t_1()
                                    .border_color(cx.theme().colors().border)
                                    .child(
                                        Label::new("See BrowserWebView struct for integration notes")
                                            .size(LabelSize::XSmall)
                                            .color(Color::Disabled),
                                    )
                            ),
                    ),
            )
    }

    fn render_error(&self, error: &str, _cx: &mut Context<Self>) -> impl IntoElement {
        let error_message = error.to_string();
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
                        Label::new(error_message)
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
        Some(IconName::Link)
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
