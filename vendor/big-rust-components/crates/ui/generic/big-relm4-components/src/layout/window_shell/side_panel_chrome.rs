// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! Side-panel chrome policy for workspace window shells.

use std::cell::RefCell;
use std::rc::Rc;

use adw::prelude::*;
use big_app_kit::window_shell::{BigWindowOpacityPercent, BigWorkspaceSidePanelChromePolicy};
use relm4::gtk;

use super::BigApplicationWindowShell;

const SIDE_PANEL_HEADER_HEIGHT_LOGICAL_PIXELS: i32 = 56;
const SIDE_PANEL_DIVIDER_WIDTH_LOGICAL_PIXELS: i32 = 1;

/// Active side-panel chrome state: the policy CSS class applied to the shell
/// widgets, its CSS text, and the single installed display provider. Refresh
/// swaps all three so a previous policy's paints (e.g. the construction-time
/// opaque bands) can never stay active under the current policy.
#[derive(Default)]
pub(super) struct SidePanelChromeRuntime {
    css_class: RefCell<Option<String>>,
    css: RefCell<String>,
    provider: RefCell<Option<gtk::CssProvider>>,
}

impl SidePanelChromeRuntime {
    fn install(&self, display: &gtk::gdk::Display) {
        if let Some(previous_provider) = self.provider.borrow_mut().take() {
            gtk::style_context_remove_provider_for_display(display, &previous_provider);
        }
        let css = self.css.borrow();
        if css.is_empty() {
            return;
        }
        let provider = gtk::CssProvider::new();
        provider.load_from_string(&css);
        gtk::style_context_add_provider_for_display(
            display,
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
        self.provider.replace(Some(provider));
    }
}

pub(super) fn apply_side_panel_chrome_policy(
    application_shell: &BigApplicationWindowShell,
    policy: BigWorkspaceSidePanelChromePolicy,
) -> (Option<gtk::Box>, Rc<SidePanelChromeRuntime>) {
    let runtime = Rc::new(SidePanelChromeRuntime::default());
    let header_side_panel = if policy.should_extend_into_header {
        application_shell.header().unparent();
        let composite_top_bar = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(0)
            .hexpand(true)
            .css_classes(["big-workspace-composite-header-bar"])
            .build();
        let header_side_panel_band = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(0)
            .width_request(side_panel_header_width(policy))
            .height_request(SIDE_PANEL_HEADER_HEIGHT_LOGICAL_PIXELS)
            .hexpand(false)
            .vexpand(true)
            .halign(gtk::Align::Start)
            .valign(gtk::Align::Fill)
            .css_classes(["big-workspace-side-panel-header-band"])
            .build();
        let header_side_panel_content = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(0)
            .width_request(policy.width_logical_pixels)
            .hexpand(false)
            .vexpand(true)
            .halign(gtk::Align::Fill)
            .valign(gtk::Align::Fill)
            .css_classes(["big-workspace-side-panel-header-content"])
            .build();
        header_side_panel_band.append(&header_side_panel_content);
        application_shell.header().set_hexpand(true);
        composite_top_bar.append(&header_side_panel_band);
        composite_top_bar.append(application_shell.header());
        application_shell.root().add_top_bar(&composite_top_bar);
        Some(header_side_panel_content)
    } else {
        None
    };

    {
        let runtime = runtime.clone();
        application_shell.root().connect_realize(move |root| {
            runtime.install(&root.display());
        });
    }
    refresh_side_panel_chrome_policy(
        &runtime,
        application_shell,
        header_side_panel.as_ref(),
        None,
        policy,
    );
    (header_side_panel, runtime)
}

pub(super) fn refresh_side_panel_chrome_policy(
    runtime: &Rc<SidePanelChromeRuntime>,
    application_shell: &BigApplicationWindowShell,
    header_side_panel: Option<&gtk::Box>,
    sidebar: Option<&gtk::Box>,
    policy: BigWorkspaceSidePanelChromePolicy,
) {
    let css_class = side_panel_chrome_css_class(policy);
    let previous_css_class = runtime.css_class.replace(Some(css_class.clone()));
    let mut policy_widgets: Vec<gtk::Widget> = vec![
        application_shell.root().clone().upcast(),
        application_shell.header().clone().upcast(),
        application_shell.body().clone().upcast(),
    ];
    if let Some(header_side_panel) = header_side_panel {
        header_side_panel.set_width_request(policy.width_logical_pixels);
        policy_widgets.push(header_side_panel.clone().upcast());
    }
    if let Some(sidebar) = sidebar {
        sidebar.set_width_request(policy.width_logical_pixels);
        policy_widgets.push(sidebar.clone().upcast());
    }
    for widget in &policy_widgets {
        if let Some(previous) = &previous_css_class
            && previous != &css_class
        {
            widget.remove_css_class(previous);
        }
        widget.add_css_class(&css_class);
    }
    runtime
        .css
        .replace(side_panel_chrome_css(&css_class, policy));
    if let Some(display) = gtk::gdk::Display::default() {
        runtime.install(&display);
    }
}

fn side_panel_chrome_css_class(policy: BigWorkspaceSidePanelChromePolicy) -> String {
    format!(
        "big-workspace-side-panel-width-{}-header-{}-side-{}-body-{}",
        policy.width_logical_pixels,
        side_panel_header_width(policy),
        policy.side_panel_opacity.value(),
        policy.body_opacity.value()
    )
}

fn side_panel_chrome_css(css_class: &str, policy: BigWorkspaceSidePanelChromePolicy) -> String {
    let side_panel_alpha = opacity_alpha(policy.side_panel_opacity);
    let body_alpha = opacity_alpha(policy.body_opacity);
    let width = policy.width_logical_pixels;
    let header_width = side_panel_header_width(policy);
    let header_background = if policy.should_extend_into_header {
        format!(
            ".{css_class} .big-workspace-composite-header-bar {{
                background-color: alpha(@window_bg_color, {body_alpha});
                background-image: none;
                padding: 0;
                margin: 0;
            }}
            .{css_class} .big-workspace-side-panel-header-band {{
                background-color: alpha(@sidebar_bg_color, {side_panel_alpha});
                background-image: none;
                min-width: {header_width}px;
                min-height: {SIDE_PANEL_HEADER_HEIGHT_LOGICAL_PIXELS}px;
                margin: 0;
                margin-left: 0;
                padding: 0;
                border-radius: 0;
                border-right: 0;
                box-shadow: none;
            }}
            .{css_class} .big-workspace-side-panel-header-content {{
                background-color: alpha(@sidebar_bg_color, {side_panel_alpha});
                background-image: none;
                min-width: {width}px;
                margin: 0;
                padding: 0;
            }}
            headerbar.{css_class}.big-application-window-header,
            .{css_class}.big-application-window-header,
            .{css_class}.big-application-window-header windowhandle,
            .{css_class} headerbar.big-application-window-header,
            .{css_class} .big-application-window-header,
            .{css_class} .big-application-window-header windowhandle {{
                padding-left: 0;
                background-color: alpha(@window_bg_color, {body_alpha});
                background-image: none;
            }}
            headerbar.{css_class}.big-application-window-header,
            .{css_class}.big-application-window-header,
            .{css_class} headerbar.big-application-window-header,
            .{css_class} .big-application-window-header {{
                border-left: {SIDE_PANEL_DIVIDER_WIDTH_LOGICAL_PIXELS}px solid @borders;
                border-radius: 0;
            }}"
        )
    } else {
        String::new()
    };

    format!(
        "{header_background}
        .{css_class} .big-workspace-side-panel-surface,
        .{css_class} .big-workspace-window-sidebar {{
            background-color: alpha(@sidebar_bg_color, {side_panel_alpha});
            background-image: none;
            min-width: {width}px;
        }}
        .{css_class} .big-workspace-side-panel-surface > viewport,
        .{css_class} .big-workspace-side-panel-content {{
            background-color: alpha(@sidebar_bg_color, {side_panel_alpha});
            background-image: none;
        }}
        .{css_class} .big-workspace-side-panel-surface {{
            border-right: 1px solid @borders;
        }}
        .big-application-window-body.{css_class},
        .{css_class} .big-application-window-body,
        .{css_class} .big-workspace-window-center,
        .{css_class} .big-workspace-window-split-pane-body {{
            background-color: alpha(@window_bg_color, {body_alpha});
        }}"
    )
}

fn side_panel_header_width(policy: BigWorkspaceSidePanelChromePolicy) -> i32 {
    policy
        .header_width_logical_pixels
        .unwrap_or(policy.width_logical_pixels + SIDE_PANEL_DIVIDER_WIDTH_LOGICAL_PIXELS)
}

fn opacity_alpha(opacity: BigWindowOpacityPercent) -> String {
    format!("{:.2}", f64::from(opacity.value()) / 100.0)
}

#[cfg(test)]
mod tests {
    use super::{side_panel_chrome_css, side_panel_header_width};
    use big_app_kit::window_shell::{BigWindowOpacityPercent, BigWorkspaceSidePanelChromePolicy};

    #[test]
    fn side_panel_chrome_css_uses_policy_width_and_opacity() {
        let policy = BigWorkspaceSidePanelChromePolicy::new(232)
            .header_width(245)
            .side_panel_opacity(BigWindowOpacityPercent::new(72).expect("valid side opacity"))
            .body_opacity(BigWindowOpacityPercent::new(91).expect("valid body opacity"));

        let css = side_panel_chrome_css("big-workspace-side-panel-test", policy);

        assert!(css.contains("232px"));
        assert_eq!(side_panel_header_width(policy), 245);
        assert!(css.contains("245px"));
        assert!(css.contains("alpha(@sidebar_bg_color, 0.72)"));
        assert!(css.contains("alpha(@window_bg_color, 0.91)"));
        assert!(css.contains(".big-workspace-composite-header-bar"));
        assert!(css.contains(
            ".big-workspace-side-panel-surface {\n            border-right: 1px solid @borders;"
        ));
        let header_band_block = css
            .split(".big-workspace-side-panel-header-band")
            .nth(1)
            .expect("header band css block")
            .split('}')
            .next()
            .expect("header band css block body");
        assert!(header_band_block.contains("border-right: 0"));
        assert!(!css.contains(".big-workspace-side-panel-header-divider"));
        assert!(!css.contains("linear-gradient"));
        assert!(css.contains("margin: 0"));
        assert!(css.contains("margin-left: 0"));
        assert!(css.contains("padding-left: 0"));
        assert!(css.contains("border-left: 1px solid @borders"));
        assert!(css.contains(".big-workspace-side-panel-surface"));
    }

    #[test]
    fn side_panel_chrome_default_header_width_aligns_header_divider() {
        let policy = BigWorkspaceSidePanelChromePolicy::new(232);

        let css = side_panel_chrome_css("big-workspace-side-panel-test", policy);

        assert_eq!(side_panel_header_width(policy), 233);
        assert!(css.contains("min-width: 233px"));
        assert!(css.contains("border-left: 1px solid @borders"));
    }
}
