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
    /// Handle used to persist config changes. None if cosmic-config was unavailable,
    /// in which case edits apply for this session only.
    config_handle: Option<cosmic_config::Config>,
    /// Contents of the add-coin field.
    coin_input: String,
    /// Set while a typed slug is being checked against the API.
    validating: bool,
    /// Why the last add attempt failed, shown under the field.
    add_error: Option<String>,
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
    /// Hand a URL to the desktop's default handler.
    OpenUrl(String),
    /// The add-coin field changed.
    CoinInputChanged(String),
    /// Check the typed slug against the API before storing it.
    AddCoin,
    /// Result of that check: the canonical slug, or why it failed.
    CoinValidated(Result<String, String>),
    /// Stop tracking a coin.
    RemoveCoin(String),
    /// A spawned side effect finished and needs no state change.
    Ignore,
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

    /// Writes the coin list through cosmic-config so it survives a restart. Without
    /// a handle the change still applies, just only for this session.
    fn persist_coins(&mut self, coins: Vec<String>) {
        match self.config_handle.as_ref() {
            Some(handle) => {
                if let Err(err) = self.config.set_coins(handle, coins) {
                    self.add_error = Some(format!("could not save: {err}"));
                }
            }
            None => self.config.coins = coins,
        }
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
        let config_handle = cosmic_config::Config::new(Self::APP_ID, Config::VERSION).ok();
        let config = config_handle
            .as_ref()
            .map(|context| match Config::get_entry(context) {
                Ok(config) => config,
                Err((_errors, config)) => config,
            })
            .unwrap_or_default();

        let mut app = AppModel {
            core,
            popup: None,
            config,
            config_handle,
            coin_input: String::new(),
            validating: false,
            add_error: None,
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

            // Only the symbol navigates. With a remove button in the same row, a
            // whole-row target would turn a near-miss on the X into an opened
            // browser tab, and a link ought to look like one.
            let symbol = widget::button::link(quote.symbol.clone())
                .padding(0)
                .on_press(Message::OpenUrl(format!(
                    "https://www.coingecko.com/en/coins/{}",
                    quote.id
                )));

            let row = widget::row::with_children(vec![
                widget::container(symbol)
                    .width(Length::Fixed(56.0))
                    .into(),
                spark,
                widget::text::body(price).width(Length::Fill).into(),
                widget::text::body(change).into(),
                widget::button::icon(widget::icon::from_name("window-close-symbolic"))
                    .extra_small()
                    .on_press(Message::RemoveCoin(quote.id.clone()))
                    .into(),
            ])
            .align_y(Alignment::Center)
            .spacing(8);

            column = column.add(cosmic::applet::padded_control(row));
        }

        // Add-coin field. Submitting on Enter as well as the button, since typing a
        // slug then reaching for the mouse is the slower path.
        let input = widget::text_input(fl!("coin-placeholder"), &self.coin_input)
            .on_input(Message::CoinInputChanged)
            .on_submit(|_| Message::AddCoin)
            .width(Length::Fill);

        let add = widget::button::standard(if self.validating {
            fl!("checking")
        } else {
            fl!("add-coin")
        })
        .on_press_maybe((!self.validating).then_some(Message::AddCoin));

        column = column.add(cosmic::applet::padded_control(
            widget::row::with_children(vec![input.into(), add.into()])
                .align_y(Alignment::Center)
                .spacing(8),
        ));

        if let Some(err) = &self.add_error {
            column = column.add(cosmic::applet::padded_control(
                widget::text::caption(err.clone()),
            ));
        }

        let refresh = widget::button::text(if self.loading {
            fl!("refreshing")
        } else {
            fl!("refresh")
        })
        .on_press_maybe((!self.loading).then_some(Message::Refresh));

        let browse = widget::button::text(fl!("browse-all"))
            .on_press(Message::OpenUrl("https://www.coingecko.com/".to_owned()));

        column = column.add(
            widget::row::with_children(vec![refresh.into(), widget::space::horizontal().into(), browse.into()])
                .align_y(Alignment::Center),
        );

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

            // tokio::process reaps the child; std::process::Command would leave a
            // zombie behind on every click.
            Message::OpenUrl(url) => {
                return cosmic::task::future(async move {
                    let _ = tokio::process::Command::new("xdg-open")
                        .arg(&url)
                        .status()
                        .await;
                    Message::Ignore
                });
            }

            Message::Ignore => {}

            Message::CoinInputChanged(value) => {
                self.coin_input = value;
                self.add_error = None;
            }

            Message::AddCoin => {
                let query = self.coin_input.trim().to_owned();
                if query.is_empty() {
                    return Task::none();
                }

                // Resolve before storing, so a typo surfaces here rather than as a
                // silently missing row. resolve() also accepts a ticker or a name,
                // since almost nobody knows CoinGecko's internal slugs.
                self.validating = true;
                self.add_error = None;
                let currency = self.config.currency.clone();
                let tracked = self.config.coins.clone();

                return cosmic::task::future(async move {
                    let result = match crypto::resolve(&query, &currency).await {
                        Ok(slug) if tracked.contains(&slug) => Err(fl!("already-tracked")),
                        Ok(slug) => Ok(slug),
                        Err(_) => Err(fl!("unknown-coin")),
                    };
                    Message::CoinValidated(result)
                });
            }

            Message::CoinValidated(result) => {
                self.validating = false;
                match result {
                    Ok(slug) => {
                        let mut coins = self.config.coins.clone();
                        coins.push(slug);
                        self.persist_coins(coins);
                        self.coin_input.clear();
                        return self.refresh();
                    }
                    Err(err) => self.add_error = Some(err),
                }
            }

            Message::RemoveCoin(slug) => {
                let coins: Vec<String> = self
                    .config
                    .coins
                    .iter()
                    .filter(|c| **c != slug)
                    .cloned()
                    .collect();
                self.persist_coins(coins);
                // Drop the row immediately rather than waiting for the next fetch.
                self.quotes.retain(|q| q.id != slug);
            }

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
