//! The ten-section settings interface.
//!
//! Layout: a fixed navigation column on the left, a scrollable content pane on
//! the right, hosted in a single resizeable `egui::Window`. Every control is
//! wired to real state — theme changes apply on the next frame, the memory page
//! shows numbers sampled from `/proc`, and diagnostics run actual probes.
//!
//! The window never blocks the app: no section performs I/O beyond a theme file
//! read, a diagnostics export or a keyring call triggered by an explicit click.

use std::time::{Duration, Instant};

use egui::{Color32, RichText};

use crate::app::{AlertKind, RustcordApp};
use crate::auth;
use crate::diagnostics;
use crate::settings::Settings;
use crate::shortcuts::{KeyChord, ShortcutAction};
use crate::sysinfo;
use crate::ui::theme::{self, AppTheme, Palette, ThemeCategory, PRESETS};

const CHECK_REFRESH: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SettingsSection {
    #[default]
    Appearance,
    Extras,
    Themes,
    Performance,
    Memory,
    Notifications,
    Shortcuts,
    Privacy,
    Diagnostics,
    About,
}

impl SettingsSection {
    pub const ALL: [SettingsSection; 10] = [
        SettingsSection::Appearance,
        SettingsSection::Extras,
        SettingsSection::Themes,
        SettingsSection::Performance,
        SettingsSection::Memory,
        SettingsSection::Notifications,
        SettingsSection::Shortcuts,
        SettingsSection::Privacy,
        SettingsSection::Diagnostics,
        SettingsSection::About,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Appearance => "Appearance",
            Self::Extras => "Extras",
            Self::Themes => "Themes",
            Self::Performance => "Performance",
            Self::Memory => "Memory",
            Self::Notifications => "Notifications",
            Self::Shortcuts => "Shortcuts",
            Self::Privacy => "Privacy",
            Self::Diagnostics => "Diagnostics",
            Self::About => "About",
        }
    }

    pub fn glyph(self) -> &'static str {
        match self {
            Self::Appearance => "◐",
            Self::Extras => "✨",
            Self::Themes => "🎨",
            Self::Performance => "⚡",
            Self::Memory => "▤",
            Self::Notifications => "🔔",
            Self::Shortcuts => "⌨",
            Self::Privacy => "🛡",
            Self::Diagnostics => "🩺",
            Self::About => "ⓘ",
        }
    }
}

/// Transient UI state for the settings window (not persisted).
#[derive(Debug, Default)]
pub struct SettingsUiState {
    pub section: SettingsSection,
    /// `None` = show every category.
    pub filter: Option<ThemeCategory>,
    pub hex_draft: String,
    /// Action currently waiting for a key press.
    pub rebinding: Option<ShortcutAction>,
    pub confirm_reset: bool,
    pub export_status: Option<String>,
    pub checks: Vec<diagnostics::Check>,
    pub checks_at: Option<Instant>,
}

pub fn show(app: &mut RustcordApp, ctx: &egui::Context) {
    if !app.show_settings {
        return;
    }

    // Clone so the sections can freely borrow `app` mutably.
    let theme = app.plugins.theme().clone();
    let mut open = true;

    egui::Window::new("Settings")
        .open(&mut open)
        .resizable(true)
        .default_width(820.0)
        .default_height(600.0)
        .min_width(700.0)
        .min_height(460.0)
        .show(ctx, |ui| {
            ui.horizontal_top(|ui| {
                ui.vertical(|ui| {
                    ui.set_width(180.0);
                    ui.add_space(4.0);
                    for section in SettingsSection::ALL {
                        let selected = app.settings_ui.section == section;
                        let text =
                            RichText::new(format!("{}   {}", section.glyph(), section.label()))
                                .size(13.0);
                        if ui.selectable_label(selected, text).clicked() {
                            app.settings_ui.section = section;
                        }
                    }
                    ui.add_space(10.0);
                    ui.separator();
                    ui.label(
                        RichText::new(format!("v{}", env!("CARGO_PKG_VERSION")))
                            .small()
                            .color(theme.muted),
                    );
                });

                ui.separator();

                ui.vertical(|ui| {
                    let width = ui.available_width();
                    egui::ScrollArea::vertical()
                        .auto_shrink([false, false])
                        .max_height(ui.available_height())
                        .show(ui, |ui| {
                            ui.set_min_width(width - 12.0);
                            let dirty = match app.settings_ui.section {
                                SettingsSection::Appearance => app.section_appearance(ui, &theme),
                                SettingsSection::Extras => app.section_extras(ui, &theme),
                                SettingsSection::Themes => app.section_themes(ui, &theme),
                                SettingsSection::Performance => app.section_performance(ui, &theme),
                                SettingsSection::Memory => app.section_memory(ui, &theme),
                                SettingsSection::Notifications => {
                                    app.section_notifications(ui, &theme)
                                }
                                SettingsSection::Shortcuts => app.section_shortcuts(ui, &theme),
                                SettingsSection::Privacy => app.section_privacy(ui, &theme),
                                SettingsSection::Diagnostics => app.section_diagnostics(ui, &theme),
                                SettingsSection::About => app.section_about(ui, &theme),
                            };
                            if dirty {
                                app.settings_changed();
                            }
                        });
                });
            });
        });

    if !open {
        app.show_settings = false;
    }
}

// ---------------------------------------------------------------------------
// Shared widgets
// ---------------------------------------------------------------------------

fn section_heading(ui: &mut egui::Ui, theme: &AppTheme, title: &str, subtitle: &str) {
    ui.add_space(4.0);
    ui.heading(RichText::new(title).size(18.0));
    ui.label(RichText::new(subtitle).small().color(theme.muted));
    ui.add_space(8.0);
}

fn key_value(ui: &mut egui::Ui, theme: &AppTheme, label: &str, value: &str) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(label).color(theme.muted).small());
        ui.label(RichText::new(value).strong());
    });
}

/// Draw a miniature chat window using a palette, so theme cards are real
/// previews rather than colour swatches.
fn paint_preview(painter: &egui::Painter, rect: egui::Rect, palette: &Palette) {
    let radius = egui::CornerRadius::same(6);
    painter.rect_filled(rect, radius, palette.background);

    let rail = egui::Rect::from_min_size(rect.min, egui::vec2(rect.width() * 0.16, rect.height()));
    painter.rect_filled(rail, radius, palette.surface_alt);

    let list = egui::Rect::from_min_size(
        egui::pos2(rail.right(), rect.top()),
        egui::vec2(rect.width() * 0.26, rect.height()),
    );
    painter.rect_filled(list, radius, palette.surface);

    // Guild dots in the rail.
    for index in 0..3 {
        let center = egui::pos2(rail.center().x, rail.top() + 14.0 + index as f32 * 16.0);
        let color = if index == 0 {
            palette.accent
        } else {
            palette.muted.gamma_multiply(0.5)
        };
        painter.circle_filled(center, 5.0, color);
    }

    // Channel rows.
    for index in 0..4 {
        let y = list.top() + 14.0 + index as f32 * 14.0;
        let width = list.width() * if index == 1 { 0.6 } else { 0.45 };
        painter.line_segment(
            [
                egui::pos2(list.left() + 8.0, y),
                egui::pos2(list.left() + 8.0 + width, y),
            ],
            egui::Stroke::new(2.0, palette.muted.gamma_multiply(0.7)),
        );
    }

    // Message lines in the content area.
    let content_left = list.right() + 10.0;
    for index in 0..5 {
        let y = rect.top() + 18.0 + index as f32 * 15.0;
        let width = (rect.right() - content_left - 12.0) * if index % 2 == 0 { 0.85 } else { 0.55 };
        painter.line_segment(
            [
                egui::pos2(content_left, y),
                egui::pos2(content_left + width, y),
            ],
            egui::Stroke::new(2.0, palette.text.gamma_multiply(0.75)),
        );
    }

    // Accent composer pill.
    let pill = egui::Rect::from_min_size(
        egui::pos2(content_left, rect.bottom() - 20.0),
        egui::vec2((rect.right() - content_left - 12.0).max(20.0), 10.0),
    );
    painter.rect_filled(pill, radius, palette.accent.gamma_multiply(0.45));
}

fn sparkline(painter: &egui::Painter, rect: egui::Rect, samples: &[u64], max: u64, color: Color32) {
    if samples.len() < 2 || max == 0 {
        return;
    }
    let step = rect.width() / (samples.len() - 1) as f32;
    let mut previous = egui::pos2(rect.left(), rect.bottom());
    for (index, value) in samples.iter().enumerate() {
        let ratio = (*value as f32 / max as f32).clamp(0.0, 1.0);
        let point = egui::pos2(
            rect.left() + index as f32 * step,
            rect.bottom() - ratio * rect.height(),
        );
        painter.line_segment([previous, point], egui::Stroke::new(1.5, color));
        previous = point;
    }
}

// ---------------------------------------------------------------------------
// Sections
// ---------------------------------------------------------------------------

impl RustcordApp {
    fn section_appearance(&mut self, ui: &mut egui::Ui, theme: &AppTheme) -> bool {
        section_heading(
            ui,
            theme,
            "Appearance",
            "Palette, accent and density. Changes apply immediately.",
        );
        let mut dirty = false;

        ui.label(RichText::new("Theme preset").strong());
        ui.add_space(2.0);
        for preset in PRESETS {
            let selected = self.settings.theme_preset == preset.id;
            ui.horizontal(|ui| {
                let (rect, _) =
                    ui.allocate_exact_size(egui::vec2(16.0, 16.0), egui::Sense::hover());
                ui.painter()
                    .rect_filled(rect, egui::CornerRadius::same(4), preset.palette.accent);
                if ui
                    .selectable_label(selected, RichText::new(preset.name).size(13.0))
                    .clicked()
                {
                    self.settings.theme_preset = preset.id.to_string();
                    self.settings.set_accent(None);
                    dirty = true;
                }
                ui.label(
                    RichText::new(preset.category.label())
                        .small()
                        .color(theme.muted),
                );
            });
        }

        ui.add_space(10.0);
        ui.separator();
        ui.label(RichText::new("Accent").strong());

        let mut accent = self.settings.accent().unwrap_or(theme.accent);
        // Sliders write an explicit override; the reset button removes it.
        let mut red = accent.r();
        let mut green = accent.g();
        let mut blue = accent.b();
        let changed = ui
            .add(egui::Slider::new(&mut red, 0..=255).text("R"))
            .changed()
            | ui.add(egui::Slider::new(&mut green, 0..=255).text("G"))
                .changed()
            | ui.add(egui::Slider::new(&mut blue, 0..=255).text("B"))
                .changed();
        if changed {
            accent = Color32::from_rgb(red, green, blue);
            self.settings.set_accent(Some(accent));
            self.settings_ui.hex_draft = crate::utils::color_to_hex(accent);
            dirty = true;
        }

        ui.horizontal(|ui| {
            ui.label("Hex");
            let mut hex = if self.settings_ui.hex_draft.is_empty() {
                crate::utils::color_to_hex(accent)
            } else {
                self.settings_ui.hex_draft.clone()
            };
            if ui
                .add(egui::TextEdit::singleline(&mut hex).desired_width(90.0))
                .changed()
            {
                self.settings_ui.hex_draft = hex.clone();
                if let Some(parsed) = crate::utils::parse_css_color(&hex) {
                    self.settings.set_accent(Some(parsed));
                    dirty = true;
                }
            }
            if ui.button("Use preset accent").clicked() {
                self.settings.set_accent(None);
                self.settings_ui.hex_draft.clear();
                dirty = true;
            }
        });

        ui.add_space(10.0);
        ui.separator();
        ui.label(RichText::new("Interface").strong());

        dirty |= ui
            .add(
                egui::Slider::new(&mut self.settings.ui_scale, 0.8..=1.6)
                    .step_by(0.05)
                    .text("Interface scale"),
            )
            .changed();

        dirty |= ui
            .checkbox(
                &mut self.settings.compact_mode,
                "Compact mode (tighter spacing)",
            )
            .changed();
        dirty |= ui
            .checkbox(
                &mut self.settings.reduce_motion,
                "Reduce motion (no animated progress)",
            )
            .changed();

        dirty |= ui
            .checkbox(
                &mut self.settings.translucent,
                "Translucent blurred wallpaper",
            )
            .changed();

        ui.horizontal(|ui| {
            ui.label(RichText::new("Wallpaper").color(theme.muted));
            let mut path = self.settings.wallpaper.clone().unwrap_or_default();
            let response = ui.add(
                egui::TextEdit::singleline(&mut path)
                    .hint_text("auto-detected — paste a path to override")
                    .desired_width(300.0),
            );
            // Commit on Enter or when focus leaves, not per keystroke: each
            // change re-detects and re-blurs the image.
            if response.lost_focus() {
                let trimmed = path.trim();
                let next = (!trimmed.is_empty()).then(|| trimmed.to_string());
                if next != self.settings.wallpaper {
                    self.settings.wallpaper = next;
                    dirty = true;
                }
            }
        });

        ui.add_space(10.0);
        ui.label(RichText::new("Preview").small().color(theme.muted));
        let (rect, _) = ui.allocate_exact_size(
            egui::vec2(ui.available_width().min(460.0), 132.0),
            egui::Sense::hover(),
        );
        let preview_theme = self.settings.theme();
        paint_preview(
            ui.painter(),
            rect,
            &Palette {
                background: preview_theme.background,
                surface: preview_theme.surface,
                surface_alt: preview_theme.surface_alt,
                accent: preview_theme.accent,
                text: preview_theme.text,
                muted: preview_theme.muted,
                danger: preview_theme.danger,
                success: preview_theme.success,
            },
        );

        dirty
    }

    fn section_extras(&mut self, ui: &mut egui::Ui, theme: &AppTheme) -> bool {
        section_heading(
            ui,
            theme,
            "Extras",
            "Vencord-style conveniences implemented natively. No Discord plugin or bundle patch is involved.",
        );
        let mut dirty = false;

        dirty |= ui
            .checkbox(
                &mut self.settings.message_timestamps,
                "Timestamps on every message",
            )
            .changed();
        ui.label(
            RichText::new(
                "Adds the day to each message row — Today, Yesterday, then the date — and keeps the exact time in the hover tooltip. With this off, rows show only the clock.",
            )
            .small()
            .color(theme.muted),
        );

        ui.add_space(8.0);
        dirty |= ui
            .checkbox(
                &mut self.settings.character_counter,
                "Character counter in the composer",
            )
            .changed();
        ui.label(
            RichText::new(
                "Counts the draft against Discord's 2000-character limit. The count turns accent-coloured near the limit and red once the message is too long to send.",
            )
            .small()
            .color(theme.muted),
        );

        ui.add_space(10.0);
        ui.separator();
        ui.label(RichText::new("Discord Rich Presence").strong());
        dirty |= ui
            .checkbox(
                &mut self.settings.discord_rpc,
                "Show what I'm doing on my Discord profile",
            )
            .changed();

        ui.horizontal(|ui| {
            ui.label(RichText::new("Application id").color(theme.muted));
            let mut client_id = self.settings.rpc_client_id.clone().unwrap_or_default();
            let response = ui.add(
                egui::TextEdit::singleline(&mut client_id)
                    .hint_text("digits from your Discord application")
                    .desired_width(240.0),
            );
            // Commit on focus loss: each change restarts the IPC worker.
            if response.lost_focus() {
                let trimmed = client_id.trim();
                let next = (!trimmed.is_empty()).then(|| trimmed.to_string());
                if next != self.settings.rpc_client_id {
                    self.settings.rpc_client_id = next;
                    dirty = true;
                }
            }
        });

        dirty |= ui
            .checkbox(
                &mut self.settings.rpc_show_channel,
                "Include the channel I'm reading",
            )
            .changed();

        ui.label(
            RichText::new(
                "Presence is sent to the Discord client already running on this machine over its local socket — no token is involved and nothing goes to a server of ours. Leave the id empty to reuse the OAuth application id.",
            )
            .small()
            .color(theme.muted),
        );

        ui.add_space(10.0);
        ui.separator();
        ui.label(RichText::new("Sandboxed WebAssembly plugins").strong());
        dirty |= ui
            .checkbox(
                &mut self.settings.wasm_plugins,
                "Load WASM plugins from the config folder",
            )
            .changed();

        let plugin_dir = crate::settings::Settings::config_dir().join("plugins");
        let loaded = self.plugins.external_count();
        ui.label(
            RichText::new(format!(
                "{loaded} plugin(s) loaded · {}",
                plugin_dir.display()
            ))
            .small()
            .color(theme.muted),
        );
        ui.label(
            RichText::new(
                "Takes effect next launch. Each folder needs plugin.json and a .wasm module; modules get no host functions, a fuel budget, a 16 MiB memory ceiling and can only rewrite message text if they declare transform_messages. Takes effect next launch.",
            )
            .small()
            .color(theme.muted),
        );

        dirty
    }

    fn section_themes(&mut self, ui: &mut egui::Ui, theme: &AppTheme) -> bool {
        section_heading(
            ui,
            theme,
            "Themes",
            "Preloaded palettes plus optional Vencord-style theme files.",
        );
        let mut dirty = false;

        ui.horizontal_wrapped(|ui| {
            let mut all_selected = self.settings_ui.filter.is_none();
            if ui.selectable_label(all_selected, "All").clicked() {
                self.settings_ui.filter = None;
                all_selected = true;
            }
            let _ = all_selected;
            for category in ThemeCategory::ALL {
                let selected = self.settings_ui.filter == Some(category);
                if ui.selectable_label(selected, category.label()).clicked() {
                    self.settings_ui.filter = if selected { None } else { Some(category) };
                }
            }
        });
        ui.add_space(6.0);

        let visible: Vec<&theme::ThemePreset> = PRESETS
            .iter()
            .filter(|preset| {
                self.settings_ui
                    .filter
                    .map(|filter| filter == preset.category)
                    .unwrap_or(true)
            })
            .collect();

        let card_width = 190.0;
        let columns = ((ui.available_width() + 12.0) / (card_width + 12.0))
            .floor()
            .max(1.0) as usize;

        for chunk in visible.chunks(columns) {
            ui.horizontal_top(|ui| {
                for preset in chunk {
                    ui.vertical(|ui| {
                        ui.set_width(card_width);
                        let selected = self.settings.theme_preset == preset.id;
                        let (rect, response) = ui.allocate_exact_size(
                            egui::vec2(card_width, 108.0),
                            egui::Sense::click(),
                        );
                        paint_preview(ui.painter(), rect, &preset.palette);
                        if selected {
                            // Plain line border: no version-specific stroke API.
                            let border = egui::Stroke::new(2.0, preset.palette.accent);
                            let corners = [
                                (rect.left_top(), rect.right_top()),
                                (rect.right_top(), rect.right_bottom()),
                                (rect.right_bottom(), rect.left_bottom()),
                                (rect.left_bottom(), rect.left_top()),
                            ];
                            for (start, end) in corners {
                                ui.painter().line_segment([start, end], border);
                            }
                        }
                        if response.clicked() {
                            self.settings.theme_preset = preset.id.to_string();
                            self.settings.set_accent(None);
                            dirty = true;
                        }
                        if response.hovered() {
                            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                        }

                        ui.horizontal(|ui| {
                            ui.label(RichText::new(preset.name).strong().color(if selected {
                                preset.palette.accent
                            } else {
                                theme.text
                            }));
                            ui.label(
                                RichText::new(preset.category.label())
                                    .small()
                                    .color(theme.muted),
                            );
                        });
                    });
                }
            });
            ui.add_space(8.0);
        }

        ui.add_space(6.0);
        ui.separator();
        ui.label(RichText::new("Theme file (Vencord-compatible)").strong());
        ui.label(
            RichText::new(
                "Only CSS custom properties are read. Selectors, @import and arbitrary CSS are \
                 ignored; nothing is injected into Discord.",
            )
            .small()
            .color(theme.muted),
        );

        match self.settings.theme_path() {
            Some(path) => {
                key_value(ui, theme, "Path", &path.display().to_string());
                if !path.exists() {
                    ui.label(
                        RichText::new("file not found — using the preset palette")
                            .small()
                            .color(theme.danger),
                    );
                }
            }
            None => {
                ui.label(
                    RichText::new("no theme file configured — preset palette is active")
                        .small()
                        .color(theme.muted),
                );
            }
        }

        ui.horizontal(|ui| {
            dirty |= ui
                .checkbox(
                    &mut self.settings.vencord_themes,
                    "Enable theme file overrides",
                )
                .changed();
            if ui.button("Reload theme file").clicked() {
                dirty = true;
                self.status_line = Some("theme file reloaded".to_string());
            }
        });

        dirty
    }

    fn section_performance(&mut self, ui: &mut egui::Ui, theme: &AppTheme) -> bool {
        section_heading(
            ui,
            theme,
            "Performance",
            "Resource priority drives repaint cadence and the message buffer size.",
        );
        let mut dirty = false;

        ui.horizontal(|ui| {
            for priority in crate::settings::ResourcePriority::ALL {
                let selected = self.settings.resource_priority == priority;
                if ui.selectable_label(selected, priority.label()).clicked() && !selected {
                    self.settings.resource_priority = priority;
                    dirty = true;
                }
            }
        });
        ui.label(
            RichText::new(self.settings.resource_priority.description())
                .small()
                .color(theme.muted),
        );

        ui.add_space(8.0);
        key_value(
            ui,
            theme,
            "Idle repaint interval",
            &format!(
                "{} ms",
                self.settings.resource_priority.repaint_interval_ms()
            ),
        );
        key_value(
            ui,
            theme,
            "Message history cap",
            &format!("{} messages", self.settings.resource_priority.history_cap()),
        );
        key_value(
            ui,
            theme,
            "Messages held now",
            &self.messages.len().to_string(),
        );
        key_value(ui, theme, "Frames drawn", &self.frames.to_string());

        ui.add_space(8.0);
        ui.horizontal(|ui| {
            if ui.button("Trim history now").clicked() {
                self.trim_history();
                self.status_line = Some("history trimmed to the configured cap".to_string());
            }
            dirty |= ui
                .checkbox(&mut self.settings.reduce_motion, "Reduce motion")
                .changed();
        });

        ui.add_space(8.0);
        ui.label(
            RichText::new(
                "Polling is event-driven: the gateway is a WebSocket, and /proc sampling is the \
                 only timer, throttled to 1 Hz while this window is open and 0.2 Hz otherwise.",
            )
            .small()
            .color(theme.muted),
        );

        dirty
    }

    fn section_memory(&mut self, ui: &mut egui::Ui, theme: &AppTheme) -> bool {
        section_heading(
            ui,
            theme,
            "Memory",
            "Sampled from /proc for this process. There is no WebView or renderer subprocess.",
        );
        let mut dirty = false;

        self.monitor.maybe_sample(Duration::from_millis(1000));
        self.monitor.refresh_top(8);

        let rss = self.monitor.rss_bytes().unwrap_or(0);
        let budget_bytes = u64::from(self.settings.memory_budget_mb) * 1024 * 1024;

        ui.horizontal(|ui| {
            ui.label(RichText::new(sysinfo::human_bytes(rss)).size(30.0).strong());
            ui.vertical(|ui| {
                ui.label(
                    RichText::new(format!("budget {} MB", self.settings.memory_budget_mb))
                        .small()
                        .color(theme.muted),
                );
                ui.label(
                    RichText::new(format!(
                        "uptime {}",
                        sysinfo::human_uptime(self.monitor.uptime())
                    ))
                    .small()
                    .color(theme.muted),
                );
            });
        });

        let ratio = if budget_bytes == 0 {
            0.0
        } else {
            (rss as f32 / budget_bytes as f32).clamp(0.0, 1.0)
        };
        let bar_color = if rss > budget_bytes {
            theme.danger
        } else {
            theme.success
        };
        ui.add(
            egui::ProgressBar::new(ratio)
                .desired_width(ui.available_width().min(420.0))
                .text(format!("{:.0}% of budget", ratio * 100.0))
                .fill(bar_color),
        );

        ui.add_space(8.0);
        let (used, cap, count, pending) = self.images.stats();
        let mut cache_mb = self.settings.image_cache_mb;
        ui.horizontal(|ui| {
            ui.label(RichText::new("Image cache").small().color(theme.muted));
            let mut detail = format!(
                "{} / {} · {count} held",
                sysinfo::human_bytes(used as u64),
                sysinfo::human_bytes(cap as u64)
            );
            if pending > 0 {
                detail.push_str(&format!(" · {pending} loading"));
            }
            ui.label(RichText::new(detail).small().color(theme.text));
        });
        if ui
            .add(
                egui::Slider::new(&mut cache_mb, 8..=512)
                    .suffix(" MB")
                    .text("budget"),
            )
            .changed()
        {
            self.settings.image_cache_mb = cache_mb;
            dirty = true;
        }
        if ui.button("Clear image cache").clicked() {
            self.images.clear();
        }

        ui.add_space(8.0);
        let history: Vec<u64> = self.monitor.history.samples().iter().copied().collect();
        let (rect, _) = ui.allocate_exact_size(
            egui::vec2(ui.available_width().min(460.0), 64.0),
            egui::Sense::hover(),
        );
        ui.painter()
            .rect_filled(rect, egui::CornerRadius::same(4), theme.surface_alt);
        let max = history
            .iter()
            .copied()
            .max()
            .unwrap_or(1)
            .max(budget_bytes / 2);
        sparkline(ui.painter(), rect, &history, max, theme.accent);

        ui.add_space(8.0);
        key_value(
            ui,
            theme,
            "Peak (kernel VmHWM)",
            &sysinfo::human_bytes(self.monitor.peak_bytes().unwrap_or(0)),
        );
        key_value(
            ui,
            theme,
            "Average (this session)",
            &sysinfo::human_bytes(self.monitor.history.average().unwrap_or(0)),
        );
        key_value(
            ui,
            theme,
            "CPU",
            &self
                .monitor
                .cpu_percent
                .map(|percent| format!("{percent:.1}%"))
                .unwrap_or_else(|| "measuring…".to_string()),
        );
        if let Some(sample) = self.monitor.sample {
            key_value(ui, theme, "Threads", &sample.threads.to_string());
            key_value(ui, theme, "File descriptors", &sample.fds.to_string());
        }
        if let Some(available) = sysinfo::system_available_bytes() {
            key_value(
                ui,
                theme,
                "System available",
                &sysinfo::human_bytes(available),
            );
        }
        key_value(
            ui,
            theme,
            "Samples buffered",
            &format!(
                "{} / {}",
                self.monitor.history.len(),
                self.monitor.history.capacity()
            ),
        );

        ui.add_space(10.0);
        ui.separator();
        dirty |= ui
            .add(
                egui::Slider::new(&mut self.settings.memory_budget_mb, 100..=800)
                    .text("Budget (MB)"),
            )
            .changed();
        dirty |= ui
            .checkbox(
                &mut self.settings.memory_alerts,
                "Warn when the budget is exceeded",
            )
            .changed();

        ui.add_space(10.0);
        ui.label(RichText::new("Largest resident processes").strong());
        if self.monitor.top.is_empty() {
            ui.label(
                RichText::new("process table unavailable")
                    .small()
                    .color(theme.muted),
            );
        }
        for entry in &self.monitor.top {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(sysinfo::human_bytes(entry.rss_bytes))
                        .monospace()
                        .color(theme.muted),
                );
                ui.label(RichText::new(&entry.name).monospace());
                ui.label(
                    RichText::new(format!("pid {}", entry.pid))
                        .small()
                        .color(theme.muted),
                );
            });
        }

        dirty
    }

    fn section_notifications(&mut self, ui: &mut egui::Ui, theme: &AppTheme) -> bool {
        section_heading(
            ui,
            theme,
            "Notifications",
            "In-app alerts only. Rustcord does not register a desktop notification daemon.",
        );
        let mut dirty = false;

        dirty |= ui
            .checkbox(
                &mut self.settings.memory_alerts,
                "Alert when the memory budget is exceeded",
            )
            .changed();
        dirty |= ui
            .checkbox(
                &mut self.settings.streaming_notifications,
                "Show streaming indicators",
            )
            .changed();
        dirty |= ui
            .checkbox(
                &mut self.settings.spotify_status_sync,
                "Show Spotify now-playing activity",
            )
            .changed();

        ui.horizontal(|ui| {
            ui.label(RichText::new("Spotify application id").color(theme.muted));
            let mut client_id = self.settings.spotify_client_id.clone().unwrap_or_default();
            let response = ui.add(
                egui::TextEdit::singleline(&mut client_id)
                    .hint_text("from the Spotify developer dashboard")
                    .desired_width(260.0),
            );
            if response.lost_focus() {
                let trimmed = client_id.trim();
                let next = (!trimmed.is_empty()).then(|| trimmed.to_string());
                if next != self.settings.spotify_client_id {
                    self.settings.spotify_client_id = next;
                    dirty = true;
                }
            }
        });

        ui.horizontal(|ui| {
            if self.spotify_connected() {
                if ui.button("Disconnect Spotify").clicked() {
                    self.disconnect_spotify();
                }
            } else {
                let busy = self.spotify_busy();
                let label = if busy {
                    "Waiting for the browser…"
                } else {
                    "Connect Spotify"
                };
                if ui
                    .add_enabled(!busy, egui::Button::new(label))
                    .on_disabled_hover_text("Finish or cancel the open browser tab first")
                    .clicked()
                {
                    self.start_spotify_connect();
                }
            }
            ui.label(
                RichText::new(&self.spotify_status)
                    .small()
                    .color(theme.muted),
            );
        });
        ui.label(
            RichText::new(
                "Connecting asks Spotify only for `user-read-currently-playing`. Tokens stay in the OS keyring and the poll runs every five seconds; the demo track is replaced by whatever is actually playing.",
            )
            .small()
            .color(theme.muted),
        );
        dirty |= ui
            .checkbox(&mut self.settings.show_blyat_banner, "Show Blyat banner")
            .changed();

        ui.add_space(8.0);
        if ui.button("Send a test alert").clicked() {
            self.push_alert(
                "This is a test alert from the Notifications page.".to_string(),
                AlertKind::Info,
                6.0,
            );
        }
        ui.label(
            RichText::new("Active alerts appear above the chat until dismissed.")
                .small()
                .color(theme.muted),
        );

        dirty
    }

    fn section_shortcuts(&mut self, ui: &mut egui::Ui, theme: &AppTheme) -> bool {
        section_heading(
            ui,
            theme,
            "Shortcuts",
            "Click a binding, then press the new key combination.",
        );
        let mut dirty = false;

        // Capture the next key event while a rebind is pending.
        if let Some(action) = self.settings_ui.rebinding {
            let captured = ui.ctx().input(|input| {
                input.events.iter().find_map(|event| match event {
                    egui::Event::Key {
                        key,
                        pressed: true,
                        modifiers,
                        ..
                    } => Some((
                        format!("{key:?}"),
                        modifiers.ctrl || modifiers.command,
                        modifiers.shift,
                        modifiers.alt,
                    )),
                    _ => None,
                })
            });

            if let Some((key, ctrl, shift, alt)) = captured {
                if key == "Escape" {
                    self.settings_ui.rebinding = None;
                } else {
                    self.settings
                        .set_chord(action, KeyChord::new(ctrl, shift, alt, key));
                    self.settings_ui.rebinding = None;
                    dirty = true;
                }
            }
        }

        for action in ShortcutAction::ALL {
            ui.horizontal(|ui| {
                ui.set_min_width(360.0);
                ui.label(RichText::new(action.label()).size(13.0));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.small_button("Reset").clicked() {
                        self.settings.reset_chord(action);
                        dirty = true;
                    }
                    let pending = self.settings_ui.rebinding == Some(action);
                    let label = if pending {
                        "press keys…".to_string()
                    } else {
                        self.settings.chord_for(action).to_config()
                    };
                    if ui.selectable_label(pending, label).clicked() {
                        self.settings_ui.rebinding = if pending { None } else { Some(action) };
                    }
                });
            });
        }

        if self.settings_ui.rebinding.is_some() {
            ui.label(
                RichText::new("Listening… press Esc to cancel.")
                    .small()
                    .color(theme.accent),
            );
        }

        let conflicts = self.settings.shortcut_conflicts();
        if !conflicts.is_empty() {
            ui.add_space(6.0);
            for (first, second) in conflicts {
                ui.label(
                    RichText::new(format!(
                        "Conflict: \"{}\" and \"{}\" share a chord",
                        first.label(),
                        second.label()
                    ))
                    .small()
                    .color(theme.danger),
                );
            }
        }

        ui.add_space(8.0);
        if ui.button("Restore defaults").clicked() {
            self.settings.reset_shortcuts();
            self.settings_ui.rebinding = None;
            dirty = true;
        }

        dirty
    }

    fn section_privacy(&mut self, ui: &mut egui::Ui, theme: &AppTheme) -> bool {
        section_heading(
            ui,
            theme,
            "Privacy",
            "Local-only storage. No telemetry, no hidden requests.",
        );
        let mut dirty = false;

        for line in [
            "Settings are stored as JSON in your config directory.",
            "The bot token lives in the OS keyring, never in settings.json or logs.",
            "No analytics, crash reporting or update pings are sent anywhere.",
            "The only network traffic is the Discord gateway and REST calls you start yourself.",
            "No Discord password is ever requested, stored or transmitted.",
        ] {
            ui.label(RichText::new(format!("• {line}")).size(12.5));
        }

        ui.add_space(10.0);
        key_value(
            ui,
            theme,
            "Config",
            &Settings::config_dir().display().to_string(),
        );
        key_value(
            ui,
            theme,
            "Data",
            &Settings::data_dir().display().to_string(),
        );

        ui.horizontal(|ui| {
            if ui.button("Open config folder").clicked() {
                if let Err(err) = open::that(Settings::config_dir()) {
                    self.status_line = Some(format!("could not open folder: {err}"));
                }
            }
            if ui.button("Open data folder").clicked() {
                let _ = std::fs::create_dir_all(Settings::data_dir());
                if let Err(err) = open::that(Settings::data_dir()) {
                    self.status_line = Some(format!("could not open folder: {err}"));
                }
            }
        });

        ui.add_space(10.0);
        dirty |= ui
            .checkbox(
                &mut self.settings.remember_history,
                "Keep message history in memory while the app runs",
            )
            .changed();

        ui.horizontal(|ui| {
            if ui.button("Clear message history").clicked() {
                self.messages.clear();
                self.history.clear();
                self.metrics.clear();
                self.metrics_channel = None;
                self.images.clear();
                self.status_line = Some("message history cleared".to_string());
            }
        });

        ui.add_space(10.0);
        ui.separator();
        ui.label(RichText::new("Runs on this PC").strong());
        ui.label(
            RichText::new(
                "No database, no telemetry, no account with anyone but Discord. Message history \
                 lives in memory and is gone when the window closes; the only files are the \
                 settings below and two keyring entries.",
            )
            .small()
            .color(theme.muted),
        );

        egui::Grid::new("local_stores")
            .num_columns(2)
            .spacing([12.0, 4.0])
            .show(ui, |ui| {
                for store in crate::privacy::STORES {
                    ui.label(RichText::new(store.name).small().color(theme.text));
                    ui.label(
                        RichText::new(format!("{} — {}", store.location, store.detail))
                            .small()
                            .color(theme.muted),
                    );
                    ui.end_row();
                }
            });

        ui.add_space(4.0);
        ui.label(
            RichText::new(format!(
                "Network: {} — every request is checked against this list, including the \
                 gateway's resume URL.",
                crate::privacy::ALLOWED_HOSTS.join(", ")
            ))
            .small()
            .color(theme.muted),
        );
        ui.label(
            RichText::new(format!(
                "History cap: {} messages · image cache: {} MB · diagnostics stay in memory \
                 until you export them.",
                self.settings.resource_priority.history_cap(),
                self.settings.image_cache_mb
            ))
            .small()
            .color(theme.muted),
        );

        ui.add_space(10.0);
        ui.separator();
        ui.label(RichText::new("Discord connection").strong());
        ui.label(
            RichText::new(
                "Bot token only. Rustcord never asks for an account password and does not use \
                 user tokens: automating a user account violates Discord's terms.",
            )
            .small()
            .color(theme.muted),
        );

        let mut token = self.token_draft.clone();
        if ui
            .add(
                egui::TextEdit::singleline(&mut token)
                    .password(true)
                    .hint_text("Bot token"),
            )
            .changed()
        {
            self.token_draft = token;
            self.token_status = "Unsaved token".to_string();
        }

        ui.horizontal(|ui| {
            if ui.button("Save token").clicked() {
                match auth::store_discord_token(self.token_draft.trim()) {
                    Ok(()) => self.token_status = "Token saved to keyring".to_string(),
                    Err(err) => self.token_status = format!("Could not save token: {err:#}"),
                }
            }
            if ui.button("Connect gateway").clicked() {
                self.restart_gateway();
            }
            if ui.button("Sign out").clicked() {
                // Drops the keyring entry and returns to the sign-in page.
                self.sign_out();
            }
        });

        ui.label(RichText::new(&self.token_status).small().color(theme.muted));
        ui.label(
            RichText::new(&self.gateway_status)
                .small()
                .color(theme.muted),
        );

        ui.add_space(10.0);
        ui.separator();
        ui.label(RichText::new("Discord identity (OAuth2, identify only)").strong());
        ui.label(
            RichText::new(
                "Separate from the bot token above. Signing in with Discord shows your \
                 username, avatar and user id; it grants no messaging, and the tokens live in \
                 the system keyring, never in settings.json.",
            )
            .small()
            .color(theme.muted),
        );

        let mut client_id = self.settings.oauth_client_id.clone().unwrap_or_default();
        ui.horizontal(|ui| {
            ui.label(RichText::new("Client ID").small().color(theme.muted));
            if ui
                .add(
                    egui::TextEdit::singleline(&mut client_id)
                        .hint_text("digits only")
                        .desired_width(180.0),
                )
                .changed()
            {
                let trimmed = client_id.trim().to_string();
                self.settings.oauth_client_id = if trimmed.is_empty() {
                    None
                } else {
                    Some(trimmed)
                };
                dirty = true;
            }
        });
        ui.label(
            RichText::new(format!(
                "Redirect URI to register: {}",
                crate::api::oauth::redirect_uri()
            ))
            .small()
            .color(theme.muted),
        );

        match self.identity.clone() {
            Some(session) => {
                ui.label(
                    RichText::new(format!(
                        "Signed in as {} (@{}) · id {}",
                        session.identity.display_name(),
                        session.identity.username,
                        session.identity.id
                    ))
                    .small()
                    .color(theme.text),
                );
                if self.identity_expired {
                    ui.label(
                        RichText::new("Session expired; sign in again to refresh it.")
                            .small()
                            .color(theme.danger),
                    );
                }
            }
            None => {
                ui.label(RichText::new("Not signed in.").small().color(theme.muted));
            }
        }

        ui.horizontal(|ui| {
            if self.oauth.pending.is_some() {
                ui.spinner();
                if ui.button("Cancel sign-in").clicked() {
                    self.cancel_identity_sign_in();
                }
            } else {
                let label = if self.identity.is_some() {
                    "Switch account"
                } else {
                    "Sign in with Discord"
                };
                if ui
                    .add_enabled(
                        self.settings.oauth_client_id.is_some(),
                        egui::Button::new(label),
                    )
                    .clicked()
                {
                    self.start_identity_sign_in();
                }
            }
            if self.identity.is_some() && ui.button("Sign out of identity").clicked() {
                self.sign_out_identity();
            }
        });

        if let Some(status) = self.oauth.status.clone() {
            ui.label(RichText::new(status).small().color(theme.muted));
        }
        if let Some(error) = self.oauth.error.clone() {
            ui.label(RichText::new(error).small().color(theme.danger));
        }

        ui.add_space(10.0);
        ui.separator();
        ui.label(RichText::new("Reset").strong());
        ui.checkbox(
            &mut self.settings_ui.confirm_reset,
            "I understand this discards all local settings",
        );
        if ui
            .add_enabled(
                self.settings_ui.confirm_reset,
                egui::Button::new("Reset all settings"),
            )
            .clicked()
        {
            self.settings = Settings::default();
            self.settings_ui.confirm_reset = false;
            self.settings_ui.hex_draft.clear();
            dirty = true;
            self.status_line = Some("settings reset to defaults".to_string());
        }

        dirty
    }

    fn section_diagnostics(&mut self, ui: &mut egui::Ui, theme: &AppTheme) -> bool {
        section_heading(
            ui,
            theme,
            "Diagnostics",
            "Local probes and the captured log ring buffer.",
        );

        let stale = self
            .settings_ui
            .checks_at
            .map(|at| at.elapsed() > CHECK_REFRESH)
            .unwrap_or(true);
        if stale || self.settings_ui.checks.is_empty() {
            self.settings_ui.checks = diagnostics::run_checks(&self.settings);
            self.settings_ui.checks_at = Some(Instant::now());
        }

        for check in &self.settings_ui.checks {
            let color = match check.status {
                diagnostics::CheckStatus::Pass => theme.success,
                diagnostics::CheckStatus::Warn => Color32::from_rgb(0xe0, 0xa8, 0x3c),
                diagnostics::CheckStatus::Fail => theme.danger,
            };
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(check.status.symbol())
                        .monospace()
                        .color(color),
                );
                ui.label(RichText::new(check.name).strong().size(12.5));
                ui.label(RichText::new(&check.detail).small().color(theme.muted));
            });
        }

        ui.add_space(8.0);
        ui.horizontal(|ui| {
            if ui.button("Re-run checks").clicked() {
                self.settings_ui.checks = diagnostics::run_checks(&self.settings);
                self.settings_ui.checks_at = Some(Instant::now());
            }
            if ui.button("Export diagnostics report").clicked() {
                match diagnostics::export(&self.settings) {
                    Ok(path) => {
                        self.settings_ui.export_status =
                            Some(format!("written to {}", path.display()));
                        self.status_line = Some("diagnostics exported".to_string());
                    }
                    Err(err) => {
                        self.settings_ui.export_status = Some(format!("export failed: {err:#}"));
                    }
                }
            }
        });
        if let Some(status) = &self.settings_ui.export_status {
            ui.label(RichText::new(status).small().color(theme.muted));
        }

        ui.add_space(10.0);
        ui.separator();
        ui.horizontal(|ui| {
            ui.label(RichText::new("Recent log").strong());
            ui.label(
                RichText::new(format!("({} dropped)", crate::logbuf::dropped()))
                    .small()
                    .color(theme.muted),
            );
        });

        let lines = crate::logbuf::snapshot();
        let start = lines.len().saturating_sub(40);
        egui::ScrollArea::vertical()
            .max_height(180.0)
            .auto_shrink([false, false])
            .stick_to_bottom(true)
            .show(ui, |ui| {
                for line in &lines[start..] {
                    ui.label(
                        RichText::new(line.rendered())
                            .monospace()
                            .size(11.0)
                            .color(theme.muted),
                    );
                }
            });

        false
    }

    fn section_about(&mut self, ui: &mut egui::Ui, theme: &AppTheme) -> bool {
        section_heading(
            ui,
            theme,
            "About",
            "A native Rust client for Discord bot accounts.",
        );

        key_value(ui, theme, "Version", env!("CARGO_PKG_VERSION"));
        key_value(ui, theme, "Renderer", "egui + glow (OpenGL), no WebView");
        key_value(
            ui,
            theme,
            "Runtime",
            "single process, no Electron, no Chromium",
        );
        key_value(ui, theme, "License", "MIT OR Apache-2.0");

        ui.add_space(10.0);
        ui.label(RichText::new("Resource profile").strong());
        for line in [
            "Idle target: 100–200 MB resident.",
            "Hard ceiling: 500 MB total application memory.",
            "Idle CPU target: under 5% with the window focused and no traffic.",
            "All figures in the Memory page are measured from /proc, not estimated.",
        ] {
            ui.label(
                RichText::new(format!("• {line}"))
                    .size(12.0)
                    .color(theme.muted),
            );
        }

        ui.add_space(10.0);
        ui.label(RichText::new("Account policy").strong());
        ui.label(
            RichText::new(
                "Rustcord connects only with bot tokens issued through the Discord developer \
                 portal, or approved OAuth2 applications. It does not log in as a human user, \
                 does not read or store tokens from the official client, and does not automate \
                 account actions. Third-party user clients are not permitted by Discord and can \
                 lead to account termination; nothing here claims to be ban-proof.",
            )
            .size(12.0)
            .color(theme.muted),
        );

        false
    }
}
