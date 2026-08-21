// SPDX-License-Identifier: MIT

use crate::config::{Config, PanelStyle};
use crate::crypto::{self, Quote};
use crate::fl;
use cosmic::cosmic_config::{self, CosmicConfigEntry};
use cosmic::iced::platform_specific::shell::wayland::commands::popup::{destroy_popup, get_popup};
use cosmic::iced::{time, window::Id, Alignment, Length, Limits, Subscription};
use cosmic::prelude::*;
use cosmic::widget;

/// Panel icon, recoloured by the panel theme.
const PANEL_ICON: &str = "io.github.zetakai.CosmicAppletCrypto-symbolic";

/// Sparkline dimensions in the popup.
const SPARK_WIDTH: u32 = 56;
const SPARK_HEIGHT: u32 = 18;

pub struct AppModel {
    core: cosmic::Core,
    popup: Option<Id>,
    config: Config,
    /// Latest successful quotes. Retained across failed refreshes so the panel keeps
    /// showing the last known prices rather than going blank.
    quotes: Vec<Quote>,
    /// Set when the most recent refresh failed while `quotes` still holds older data.
    stale: bool,
    /// Set when there is nothing to show at all.
    error: Option<String>,
    loading: bool,
}

#[derive(Debug, Clone)]
pub enum Message {
    TogglePopup,
    PopupClosed(Id),
    UpdateConfig(Config),
    /// Timer fired, or the user pressed refresh.
    Refresh,
    /// A fetch finished, successfully or not.
    Fetched(Result<Vec<Quote>, String>),
}

impl AppModel {
    /// Builds the panel label from the tracked coin, honouring the configured style.
    fn panel_label(&self) -> Option<String> {
        let coin = self.config.effective_panel_coin()?;
        let quote = self.quotes.iter().find(|q| &q.id == coin)?;
        let prefix = crypto::currency_prefix(&self.config.currency);

        let label = match self.config.panel_style {
            PanelStyle::Full => {
                let mut s = format!("{} {prefix}{}", quote.symbol, crypto::format_amount(quote.price));
                if let Some(change) = quote.change {
                    s.push_str(&format!(" {}", crypto::format_change(change, false)));
                }
                s
            }
            PanelStyle::Minimal => format!("{prefix}{}", crypto::format_compact(quote.price)),
            PanelStyle::Icon | PanelStyle::Compact => {
                let mut s = format!("{prefix}{}", crypto::format_compact(quote.price));
                if let Some(change) = quote.change {
                    s.push_str(&format!(" {}", crypto::format_change(change, true)));
                }
                s
            }
        };

        Some(if self.stale {
            format!("{label} {}", fl!("stale"))
        } else {
            label
        })
    }

    /// Kicks off a fetch for every configured coin.
    fn refresh(&mut self) -> Task<cosmic::Action<Message>> {
        if self.loading {
            return Task::none();
        }
        self.loading = true;
        let coins = self.config.coins.clone();
        let currency = self.config.currency.clone();
        cosmic::task::future(async move { Message::Fetched(crypto::fetch(&coins, &currency).await) })
    }
}

impl cosmic::Application for AppModel {
    type Executor = cosmic::executor::Default;
    type Flags = ();
    type Message = Message;

    const APP_ID: &'static str = "io.github.zetakai.CosmicAppletCrypto";

    fn core(&self) -> &cosmic::Core {
        &self.core
    }

    fn core_mut(&mut self) -> &mut cosmic::Core {
        &mut self.core
    }

    fn init(core: cosmic::Core, _flags: Self::Flags) -> (Self, Task<cosmic::Action<Self::Message>>) {
        let config = cosmic_config::Config::new(Self::APP_ID, Config::VERSION)
            .map(|context| match Config::get_entry(&context) {
                Ok(config) => config,
                Err((_errors, config)) => config,
            })
            .unwrap_or_default();

        let mut app = AppModel {
            core,
            popup: None,
            config,
            quotes: Vec::new(),
            stale: false,
            error: None,
            loading: false,
        };

        // Fetch immediately so the panel is populated before the first timer tick.
        let task = app.refresh();
        (app, task)
    }

    fn on_close_requested(&self, id: Id) -> Option<Message> {
        Some(Message::PopupClosed(id))
    }

    fn view(&self) -> Element<'_, Self::Message> {
        let applet = &self.core.applet;
        let horizontal = applet.is_horizontal();

        // On a vertical panel the applet's width *is* the panel's thickness, so a
        // text label would force the whole bar wider. There the icon stands alone
        // and the prices live in the popup. An explicitly chosen text style is
        // still honoured — that is the user accepting the width.
        let icon_only = !horizontal && self.config.panel_style == PanelStyle::Icon;

        let label = self.panel_label();

        // The plain icon and plain text cases have applet helpers that already size
        // and centre the button correctly; only the icon-plus-text row has to be
        // assembled by hand.
        if icon_only || label.is_none() {
            return applet
                .icon_button(PANEL_ICON)
                .on_press(Message::TogglePopup)
                .into();
        }

        let label = label.unwrap_or_default();

        if self.config.panel_style != PanelStyle::Icon {
            return applet
                .text_button(applet.text(label), Message::TogglePopup)
                .into();
        }

        // Icon + text on a horizontal panel. Mirrors what button_from_element does,
        // minus its fixed width, which would clip the label.
        let (major, minor) = applet.suggested_padding(true);
        let (horizontal_padding, vertical_padding) =
            if horizontal { (major, minor) } else { (minor, major) };
        let suggested = applet.suggested_size(true);

        let content = widget::row::with_children(vec![
            widget::icon::from_name(PANEL_ICON)
                .symbolic(true)
                .size(suggested.0)
                .into(),
            applet.text(label).into(),
        ])
        .spacing(4)
        .align_y(Alignment::Center);

        widget::button::custom(
            widget::layer_container::layer_container(content)
                .center_y(Length::Fill)
                .center_x(Length::Shrink),
        )
        .height(Length::Fixed(f32::from(suggested.1 + 2 * vertical_padding)))
        .padding([0, horizontal_padding])
        .on_press(Message::TogglePopup)
        .class(cosmic::theme::Button::AppletIcon)
        .into()
    }

    fn view_window(&self, _id: Id) -> Element<'_, Self::Message> {
        let mut column = widget::list_column();

        if let Some(error) = &self.error {
            column = column.add(widget::text::body(error.clone()));
        }

        for quote in &self.quotes {
            let prefix = crypto::currency_prefix(&self.config.currency);
            let price = format!("{prefix}{}", crypto::format_amount(quote.price));
            let change = quote
                .change
                .map(|c| crypto::format_change(c, false))
                .unwrap_or_default();

            // A 7-day sparkline, drawn as generated SVG so no canvas feature is
            // needed. Not symbolic: its colour carries the trend direction and must
            // survive theming.
            let spark: Element<'_, Self::Message> =
                match crypto::sparkline_svg(&quote.sparkline, SPARK_WIDTH, SPARK_HEIGHT) {
                    // from_svg_bytes yields a non-symbolic handle, so the stroke
                    // colour in the generated SVG is preserved rather than themed.
                    Some(bytes) => widget::icon(widget::icon::from_svg_bytes(bytes))
                        .width(Length::Fixed(f32::from(SPARK_WIDTH as u16)))
                        .height(Length::Fixed(f32::from(SPARK_HEIGHT as u16)))
                        .into(),
                    // Keep the column aligned when a coin has no series.
                    None => widget::space::horizontal()
                        .width(Length::Fixed(f32::from(SPARK_WIDTH as u16)))
                        .into(),
                };

            let row = widget::row::with_children(vec![
                widget::text::body(quote.symbol.clone())
                    .width(Length::Fixed(56.0))
                    .into(),
                spark,
                widget::text::body(price).width(Length::Fill).into(),
                widget::text::body(change).into(),
            ])
            .align_y(Alignment::Center)
            .spacing(8);

            column = column.add(row);
        }

        let footer = widget::button::text(if self.loading {
            fl!("refreshing")
        } else {
            fl!("refresh")
        })
        .on_press_maybe((!self.loading).then_some(Message::Refresh));

        column = column.add(footer);

        self.core.applet.popup_container(column).into()
    }

    fn subscription(&self) -> Subscription<Self::Message> {
        Subscription::batch(vec![
            time::every(self.config.refresh_interval()).map(|_| Message::Refresh),
            self.core()
                .watch_config::<Config>(Self::APP_ID)
                .map(|update| Message::UpdateConfig(update.config)),
        ])
    }

    fn update(&mut self, message: Self::Message) -> Task<cosmic::Action<Self::Message>> {
        match message {
            Message::Refresh => return self.refresh(),

            Message::Fetched(result) => {
                self.loading = false;
                match result {
                    Ok(quotes) if !quotes.is_empty() => {
                        self.quotes = quotes;
                        self.stale = false;
                        self.error = None;
                    }
                    // Keep whatever we already had; only flag it as stale.
                    Ok(_) | Err(_) => {
                        let reason = match result {
                            Err(e) => e,
                            _ => "no quotes returned".to_owned(),
                        };
                        if self.quotes.is_empty() {
                            self.error = Some(reason);
                        } else {
                            self.stale = true;
                        }
                    }
                }
            }

            Message::UpdateConfig(config) => {
                let refetch = config.coins != self.config.coins
                    || config.currency != self.config.currency;
                self.config = config;
                if refetch {
                    return self.refresh();
                }
            }

            Message::TogglePopup => {
                return if let Some(p) = self.popup.take() {
                    destroy_popup(p)
                } else {
                    let new_id = Id::unique();
                    self.popup.replace(new_id);
                    let mut popup_settings = self.core.applet.get_popup_settings(
                        self.core.main_window_id().unwrap(),
                        new_id,
                        None,
                        None,
                        None,
                    );
                    popup_settings.positioner.size_limits = Limits::NONE
                        .max_width(372.0)
                        .min_width(300.0)
                        .min_height(100.0)
                        .max_height(1080.0);
                    get_popup(popup_settings)
                }
            }

            Message::PopupClosed(id) => {
                if self.popup.as_ref() == Some(&id) {
                    self.popup = None;
                }
            }
        }

        Task::none()
    }

    fn style(&self) -> Option<cosmic::iced::theme::Style> {
        Some(cosmic::applet::style())
    }
}
