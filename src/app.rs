// SPDX-License-Identifier: MIT

use crate::config::{Config, PanelStyle};
use crate::crypto::{self, Quote};
use crate::fl;
use cosmic::cosmic_config::{self, CosmicConfigEntry};
use cosmic::iced::platform_specific::shell::wayland::commands::popup::{destroy_popup, get_popup};
use cosmic::iced::{time, window::Id, Alignment, Length, Limits, Radians, Rotation, Subscription};
use std::time::{Duration, Instant};
use cosmic::prelude::*;
use cosmic::widget;

/// Panel icon, recoloured by the panel theme.
const PANEL_ICON: &str = "io.github.zetakai.CosmicAppletCrypto-symbolic";

/// Colour for a change value, or the default when there is no figure to show.
fn change_class(change: Option<f64>) -> cosmic::theme::Text {
    match change {
        Some(c) if c > 0.0 => cosmic::theme::Text::Custom(up_colour),
        Some(c) if c < 0.0 => cosmic::theme::Text::Custom(down_colour),
        _ => cosmic::theme::Text::Default,
    }
}

/// Text::Custom takes a plain fn pointer, so these cannot be closures over the
/// theme; they read it themselves when asked to style.
fn up_colour(theme: &cosmic::Theme) -> cosmic::iced::widget::text::Style {
    cosmic::iced::widget::text::Style {
        color: Some(theme.cosmic().success.base.into()),
        // Match what the theme uses for selected text elsewhere.
        selected_fill: theme.cosmic().accent.base.into(),
    }
}

fn down_colour(theme: &cosmic::Theme) -> cosmic::iced::widget::text::Style {
    cosmic::iced::widget::text::Style {
        color: Some(theme.cosmic().destructive.base.into()),
        selected_fill: theme.cosmic().accent.base.into(),
    }
}

/// The generated SVG needs its stroke as a hex string.
fn srgba_to_hex(c: cosmic::cosmic_theme::palette::Srgba) -> String {
    let channel = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    format!(
        "#{:02x}{:02x}{:02x}",
        channel(c.red),
        channel(c.green),
        channel(c.blue)
    )
}

/// Footprint of the extra-small remove button: a 16px symbolic glyph plus
/// space_xxs of padding on every side. The idle slot reserves the same box so rows
/// keep their height and position when edit mode is toggled.
const REMOVE_SLOT: f32 = 16.0 + 2.0 * 8.0;

/// Arrows sit in their own narrow column so they align down the list; the value
/// column is wide enough for the realistic worst case, "-99.9%".
const ARROW_COL_WIDTH: f32 = 14.0;
const CHANGE_COL_WIDTH: f32 = 48.0;

/// Rough advance width of the popup's body font, used to size the price column to
/// its content. Approximate is fine: the column is padded either way.
const CHAR_WIDTH: f32 = 8.5;

/// Floor for the price column so short prices still leave the numbers aligned.
const MIN_PRICE_CHARS: usize = 9;

/// Matches button::standard's own text metrics so the add field lines up with it.
const FIELD_FONT_SIZE: u16 = 14;
const FIELD_LINE_HEIGHT: u16 = 20;

/// How often the spinner advances, and by how much per tick.
const SPIN_INTERVAL: Duration = Duration::from_millis(40);
const SPIN_STEP: f32 = 0.22;

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
    /// Current angle of the spinning refresh icon, in radians.
    spin: f32,
    /// When the prices currently on screen were fetched. Approximated from the
    /// cache's age at startup so the age is right across a restart.
    data_time: Option<Instant>,
    /// Whether the coin-management controls are showing. Kept off by default so the
    /// popup is just prices until editing is actually wanted.
    editing: bool,
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
    /// Show or hide the coin-management controls.
    ToggleEdit,
    /// Advance the refresh spinner.
    SpinTick,
    /// A spawned side effect finished and needs no state change.
    Ignore,
}

impl AppModel {
    /// Builds the panel label from the tracked coin, honouring the configured style.
    fn panel_label(&self) -> Option<String> {
        let coin = self.config.effective_panel_coin()?;
        let quote = self.quotes.iter().find(|q| q.id == coin)?;
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

    /// "updated 2m ago" — answers whether what is on screen is current, which a
    /// momentary success tick does not.
    fn freshness_label(&self) -> Option<String> {
        let secs = self.data_time?.elapsed().as_secs();
        Some(match secs {
            0..=59 => fl!("updated-now"),
            60..=3599 => fl!("updated-min", n = (secs / 60).to_string()),
            _ => fl!("updated-hr", n = (secs / 3600).to_string()),
        })
    }

    /// Kicks off a fetch for every configured coin.
    fn refresh(&mut self) -> Task<cosmic::Action<Message>> {
        if self.loading {
            return Task::none();
        }
        self.loading = true;
        let coins = self.config.effective_coins();
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

        // Load the previous run's prices so the panel and popup are populated
        // instantly, without waiting on — or depending on — a network round trip.
        let cached = crypto::load_cache();
        let interval = config.refresh_interval().as_secs();
        let (quotes, cache_age) = match cached {
            Some((quotes, age)) => (quotes, Some(age)),
            None => (Vec::new(), None),
        };

        // Only reach for the network if the cache is missing or already due. A panel
        // restart should not cost a request, which is what turns a burst of restarts
        // into a rate limit.
        let needs_fetch = cache_age.is_none_or(|age| age >= interval);

        let mut app = AppModel {
            core,
            popup: None,
            config,
            config_handle,
            coin_input: String::new(),
            validating: false,
            add_error: None,
            editing: false,
            spin: 0.0,
            data_time: cache_age
                .and_then(|age| Instant::now().checked_sub(Duration::from_secs(age))),
            stale: cache_age.is_some_and(|age| age >= interval),
            quotes,
            error: None,
            loading: false,
        };

        let task = if needs_fetch { app.refresh() } else { Task::none() };
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
        // Take the up/down colours from the desktop theme rather than fixing them
        // here, so they track light and dark and any accent the user has set. The
        // SVG needs them as hex because it is generated as text.
        let palette = cosmic::theme::active().cosmic().clone();
        let up_hex = srgba_to_hex(palette.success.base);
        let down_hex = srgba_to_hex(palette.destructive.base);

        let row_spacing = cosmic::theme::spacing();
        let prefix = crypto::currency_prefix(&self.config.currency);
        let can_remove = self.quotes.len() > 1;

        // Size the price column to the widest value actually on screen rather than
        // filling the popup. A Fill column would pin the popup to its maximum width
        // regardless of content; this way a list of ordinary coins stays narrow and
        // only widens for something like SHIB at eight decimals, or IDR prices.
        let price_strings: Vec<String> = self
            .quotes
            .iter()
            .map(|q| format!("{prefix}{}", crypto::format_amount(q.price)))
            .collect();
        let price_width = price_strings
            .iter()
            .map(|p| p.chars().count())
            .max()
            .unwrap_or(0)
            .max(MIN_PRICE_CHARS) as f32
            * CHAR_WIDTH;

        let mut column = widget::list_column();

        if let Some(error) = &self.error {
            column = column.add(widget::text::caption(error.clone()));
        }

        for quote in &self.quotes {
            // Only the symbol navigates. With a remove button in the same row, a
            // whole-row target would turn a near-miss on the X into an opened
            // browser tab, and a link ought to look like one.
            let symbol = widget::button::link(quote.symbol.clone())
                .padding(0)
                .on_press(Message::OpenUrl(format!(
                    "https://www.coingecko.com/en/coins/{}",
                    quote.id
                )));

            // A 7-day sparkline, drawn as generated SVG so no canvas feature is
            // needed. Not symbolic: its colour carries the trend and must survive
            // theming.
            let rising_week = crypto::is_rising(&quote.sparkline);
            let spark_colour = if rising_week { &up_hex } else { &down_hex };

            let spark: Element<'_, Self::Message> =
                match crypto::sparkline_svg(
                    &quote.sparkline,
                    SPARK_WIDTH,
                    SPARK_HEIGHT,
                    spark_colour,
                ) {
                    Some(bytes) => widget::icon(widget::icon::from_svg_bytes(bytes))
                        .width(Length::Fixed(f32::from(SPARK_WIDTH as u16)))
                        .height(Length::Fixed(f32::from(SPARK_HEIGHT as u16)))
                        .into(),
                    // Keep the column aligned when a coin has no series.
                    None => widget::space::horizontal()
                        .width(Length::Fixed(f32::from(SPARK_WIDTH as u16)))
                        .into(),
                };

            let mut cells: Vec<Element<'_, Self::Message>> = vec![
                widget::container(symbol).width(Length::Fixed(48.0)).into(),
                spark,
                widget::container(
                    widget::text::body(format!("{prefix}{}", crypto::format_amount(quote.price)))
                        .align_x(Alignment::End),
                )
                .width(Length::Fixed(price_width))
                .into(),
                // Arrow and percentage occupy separate fixed columns. Rendered as
                // one right-aligned string the arrows drift horizontally, because
                // the number beside them varies in width.
                //
                // Both are coloured by the 24h direction, which is deliberately
                // independent of the sparkline's week-long one — a coin can be up on
                // the day and down on the week, and hiding that would be less honest
                // than showing two colours.
                widget::container(
                    widget::text::body(
                        quote.change.map(crypto::change_arrow).unwrap_or(""),
                    )
                    .class(change_class(quote.change))
                    .align_x(Alignment::Center),
                )
                .width(Length::Fixed(ARROW_COL_WIDTH))
                .into(),
                widget::container(
                    widget::text::body(
                        quote
                            .change
                            .map(|c| crypto::format_change_value(c, true))
                            .unwrap_or_default(),
                    )
                    .class(change_class(quote.change))
                    .align_x(Alignment::End),
                )
                .width(Length::Fixed(CHANGE_COL_WIDTH))
                .into(),
            ];

            // Remove buttons only exist while editing, so the resting popup is not
            // a wall of X's. The idle slot reserves the same box so toggling edit
            // mode moves nothing.
            //
            // The glyph is text rather than a themed icon: icon::from_name resolves
            // through the icon theme at runtime and gives back nothing visible when
            // the lookup misses, which fails silently and looks like a missing
            // feature. A character always draws.
            cells.push(if self.editing {
                widget::button::text("\u{2715}")
                    .padding([0, row_spacing.space_xxs])
                    .class(cosmic::theme::Button::Destructive)
                    .on_press_maybe(can_remove.then(|| Message::RemoveCoin(quote.id.clone())))
                    .into()
            } else {
                widget::space::horizontal()
                    .width(Length::Fixed(REMOVE_SLOT))
                    .height(Length::Fixed(REMOVE_SLOT))
                    .into()
            });

            column = column.add(
                widget::row::with_children(cells)
                    .align_y(Alignment::Center)
                    .spacing(8),
            );
        }

        if self.editing {
            // button::standard is a fixed space_l tall while text_input sizes itself
            // from its padding, so the two do not line up by default. The input's
            // vertical padding is derived from that height to match it.
            // space_l (32) is taller than 20px text needs; space_m keeps the row
            // compact while still clearing the text and its focus ring.
            let field_height = row_spacing.space_m;
            let vertical_pad = field_height.saturating_sub(FIELD_LINE_HEIGHT) / 2;

            let input = widget::text_input(fl!("coin-placeholder"), &self.coin_input)
                .on_input(Message::CoinInputChanged)
                .on_submit(|_| Message::AddCoin)
                .size(FIELD_FONT_SIZE)
                .padding([vertical_pad, row_spacing.space_xs])
                .width(Length::Fill);

            let add = widget::button::standard(if self.validating {
                fl!("checking")
            } else {
                fl!("add-coin")
            })
            .height(Length::Fixed(f32::from(field_height)))
            .on_press_maybe((!self.validating).then_some(Message::AddCoin));

            column = column.add(
                widget::row::with_children(vec![input.into(), add.into()])
                    .align_y(Alignment::Center)
                    .spacing(8),
            );

            if let Some(err) = &self.add_error {
                column = column.add(widget::text::caption(err.clone()));
            }
        }

        // Every footer icon button is built the same way so none of them changes
        // size between states — mixing button::icon with button::custom made the
        // row jump the moment the spinner started.
        let icon_pad = cosmic::theme::spacing().space_xxs;
        let icon_button = |name: &'static str, angle: f32, msg: Option<Message>| {
            widget::button::custom(
                widget::icon(widget::icon::from_name(name).handle())
                    .size(16)
                    // Floating turns the glyph without disturbing the layout.
                    .rotation(Rotation::Floating(Radians(angle))),
            )
            .class(cosmic::theme::Button::Icon)
            .padding(icon_pad)
            .on_press_maybe(msg)
        };

        // Two states, not three: spinning while in flight, then straight back to a
        // refresh button. A success tick would say nothing the numbers do not.
        let refresh: Element<'_, Self::Message> = if self.loading {
            // No message while in flight, which also prevents stacking requests.
            icon_button("view-refresh-symbolic", self.spin, None).into()
        } else {
            icon_button("view-refresh-symbolic", 0.0, Some(Message::Refresh)).into()
        };

        let browse = widget::button::link(fl!("browse-all"))
            .padding(0)
            .on_press(Message::OpenUrl("https://www.coingecko.com/".to_owned()));

        // A plus reads as "add a coin", which is the reason to open this. Collapsing
        // uses an arrow rather than an X so it cannot be mistaken for the per-row
        // remove buttons it sits beside.
        let edit = icon_button(
            if self.editing { "go-up-symbolic" } else { "list-add-symbolic" },
            0.0,
            Some(Message::ToggleEdit),
        );

        column = column.add(
            widget::row::with_children(vec![
                refresh,
                widget::text::caption(
                    self.freshness_label().unwrap_or_else(|| fl!("never-updated")),
                )
                .into(),
                widget::space::horizontal().into(),
                browse.into(),
                edit.into(),
            ])
            .align_y(Alignment::Center)
            .spacing(8),
        );

        self.core.applet.popup_container(column).into()
    }

    fn subscription(&self) -> Subscription<Self::Message> {
        let mut subscriptions = vec![
            time::every(self.config.refresh_interval()).map(|_| Message::Refresh),
            self.core()
                .watch_config::<Config>(Self::APP_ID)
                .map(|update| Message::UpdateConfig(update.config)),
        ];

        // Only tick while a request is actually in flight; otherwise the applet
        // sits idle rather than animating nothing.
        if self.loading {
            subscriptions.push(time::every(SPIN_INTERVAL).map(|_| Message::SpinTick));
        }

        Subscription::batch(subscriptions)
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

            Message::SpinTick => {
                if self.loading {
                    self.spin = (self.spin + SPIN_STEP) % std::f32::consts::TAU;
                }
            }

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

            Message::ToggleEdit => {
                self.editing = !self.editing;
                self.add_error = None;
                self.coin_input.clear();
            }

            Message::RemoveCoin(slug) => {
                let coins: Vec<String> = self
                    .config
                    .effective_coins()
                    .into_iter()
                    .filter(|c| *c != slug)
                    .collect();

                // An empty list would leave the applet blank; keep the last coin.
                if coins.is_empty() {
                    self.add_error = Some(fl!("keep-one-coin"));
                    return Task::none();
                }
                self.persist_coins(coins);
                // Drop the row immediately rather than waiting for the next fetch.
                self.quotes.retain(|q| q.id != slug);
            }

            Message::Fetched(result) => {
                self.loading = false;
                self.spin = 0.0;
                match result {
                    Ok(quotes) if !quotes.is_empty() => {
                        crypto::save_cache(&quotes);
                        self.data_time = Some(Instant::now());
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
                    // The popup sizes to its content between these bounds. The upper
                    // one has to clear the widest realistic row: a sub-cent coin
                    // needs eight decimals, and a high-denomination currency like
                    // IDR renders prices such as Rp1,360,446,498.
                    popup_settings.positioner.size_limits = Limits::NONE
                        .max_width(500.0)
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
