// Rusty Solitaire - main.rs
//
// The application window: wires the game engine and board to the Slint UI.
// Takes the place of window.c and ar-application.c.
//
// Copyright (C) 1998, 2003 Jonathan Blandford <jrb@mit.edu>
// Copyright (C) 2007 Christian Persch
// Copyright (C) 2026 The Rusty Solitaire authors (Rust port)
// SPDX-License-Identifier: GPL-3.0-or-later

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod board;
mod card;
mod game;
#[cfg(test)]
mod game_tests;
mod help;
mod rules;
mod scheme;
mod settings;
mod sound;
mod theme;

use std::cell::RefCell;
use std::rc::Rc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use slint::{ComponentHandle, ModelRc, Rgba8Pixel, SharedPixelBuffer, SharedString, Timer, TimerMode, VecModel};

use board::{Board, Cursor, Key};
use game::{Game, GameState};
use settings::Settings;
use sound::Sound;
use theme::{CardImages, CardTheme};

slint::include_modules!();

/// How long to wait after the last resize before re-rendering the card theme.
const RESIZE_DEBOUNCE: Duration = Duration::from_millis(80);
/// DELAYED_CALLBACK_DELAY in game.c
const AUTOPLAY_DELAY: Duration = Duration::from_millis(50);
/// How long a finger must rest on a card to show it (a mouse right-click).
const LONG_PRESS: Duration = Duration::from_millis(500);
/// The maximum number of recent games saved (MAX_RECENT in window.c).
const MAX_RECENT: usize = 5;

const DEFAULT_GAME: &str = "klondike";
const APP_NAME: &str = "Rusty Solitaire";
/// Desktop/Flatpak application ID (reverse-DNS of github.com/rustysolitaire).
const APP_ID: &str = "io.github.rustysolitaire.RustySolitaire";

struct App {
    game: Game,
    board: Board,
    theme: CardTheme,
    images: Option<CardImages>,
    settings: Settings,
    show_toolbar: bool,
    show_statusbar: bool,
    sound: bool,
    fullscreen: bool,
    dialog: Option<Dialog>,
    shown_end_dialog: bool,
    recent: Vec<String>,
}

#[derive(Clone, Copy, PartialEq)]
enum Dialog {
    Hint,
    GameOver,
    About,
    Error,
    Chooser,
    Statistics,
    Shortcuts,
}

/// Wins, games played, and the best and worst winning times in seconds
/// (AisleriotStatistic), kept per game.
#[derive(Clone, Copy, Default, Debug, PartialEq)]
struct Statistic {
    wins: u32,
    total: u32,
    best: u32,
    worst: u32,
}

impl Statistic {
    fn parse(text: &str) -> Self {
        let mut n = text.split(',').map(|v| v.trim().parse::<u32>().unwrap_or(0));
        let mut next = || n.next().unwrap_or(0);
        Statistic { wins: next(), total: next(), best: next(), worst: next() }
    }

    fn to_text(self) -> String {
        format!("{},{},{},{}", self.wins, self.total, self.best, self.worst)
    }

    /// Count a finished or abandoned game (update_statistics).
    fn add(&mut self, won: bool, seconds: u32) {
        self.total += 1;
        if won {
            self.wins += 1;
            if seconds > 0 {
                if self.best == 0 || seconds < self.best {
                    self.best = seconds;
                }
                self.worst = self.worst.max(seconds);
            }
        }
    }
}

/// minutes:seconds, or N/A (aisleriot_stats_dialog_update).
fn stat_time(seconds: u32) -> String {
    if seconds == 0 { "N/A".into() } else { format!("{}:{:02}", seconds / 60, seconds % 60) }
}

fn random_seed() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_nanos() as u64).unwrap_or(0x5eed)
}

fn item(id: &str, label: &str, accel: &str, enabled: bool) -> MenuItemData {
    MenuItemData {
        id: id.into(),
        label: label.into(),
        accel: accel.into(),
        enabled,
        kind: MenuItemKind::Normal,
        checked: false,
    }
}

fn check(id: &str, label: &str, accel: &str, checked: bool) -> MenuItemData {
    MenuItemData { kind: MenuItemKind::Check, checked, ..item(id, label, accel, true) }
}

fn separator() -> MenuItemData {
    MenuItemData { kind: MenuItemKind::Separator, ..item("", "", "", false) }
}

fn submenu(id: &str, label: &str, enabled: bool) -> MenuItemData {
    MenuItemData { kind: MenuItemKind::Submenu, ..item(id, label, "", enabled) }
}

fn menu(title: &str, items: Vec<MenuItemData>) -> MenuData {
    MenuData { title: title.into(), items: ModelRc::new(VecModel::from(items)) }
}

fn options_key(file: &str) -> String {
    format!("{file}.options")
}

fn statistics_key(file: &str) -> String {
    format!("{file}.statistics")
}

impl App {
    fn new(settings: Settings) -> Self {
        let theme_name = settings.get_string("card-theme").unwrap_or(theme::DEFAULT_THEME).to_string();
        let theme = CardTheme::load(&theme_name)
            .or_else(|| CardTheme::load(theme::DEFAULT_THEME))
            .expect("default card theme is embedded");
        let mut board = Board::new();
        board.click_to_move = settings.get_bool("click-to-move", false);
        let recent: Vec<String> = settings
            .get_string("recent-games")
            .map(|s| s.split(',').filter(|g| !g.is_empty()).map(String::from).collect())
            .unwrap_or_default();
        let file = settings.get_string("game").unwrap_or(DEFAULT_GAME).to_string();
        let game = Self::load_game(&settings, &file)
            .or_else(|_| Self::load_game(&settings, DEFAULT_GAME))
            .expect("Klondike always loads");
        let mut app = App {
            game,
            board,
            theme,
            images: None,
            show_toolbar: settings.get_bool("show-toolbar", true),
            show_statusbar: settings.get_bool("show-statusbar", true),
            sound: settings.get_bool("sound", true),
            settings,
            fullscreen: false,
            dialog: None,
            shown_end_dialog: false,
            recent,
        };
        app.remember_game();
        app
    }

    /// Load a game and apply its saved options before the first deal.
    fn load_game(settings: &Settings, file: &str) -> scheme::Result<Game> {
        let mut game = Game::new(file, random_seed())?;
        if let Some(saved) = settings.get_string(&options_key(file)) {
            let values: Vec<bool> = saved.split(',').map(|v| v == "1").collect();
            if values.len() == game.options().len() {
                game.set_options(&values);
                game.new_game(random_seed())?;
            }
        }
        Ok(game)
    }

    /// Save the current game as the one to open next time, and put it at
    /// the front of the recently played list.
    fn remember_game(&mut self) {
        let file = self.game.file().to_string();
        self.recent.retain(|g| *g != file);
        self.recent.insert(0, file.clone());
        self.recent.truncate(MAX_RECENT);
        self.settings.set_string("game", &file);
        self.settings.set_string("recent-games", &self.recent.join(","));
        self.settings.save();
    }

    fn save_options(&mut self) {
        let values: Vec<&str> = self.game.options().iter().map(|o| if o.value { "1" } else { "0" }).collect();
        let key = options_key(self.game.file());
        self.settings.set_string(&key, &values.join(","));
        self.settings.save();
    }

    fn statistic(&self) -> Statistic {
        Statistic::parse(self.settings.get_string(&statistics_key(self.game.file())).unwrap_or(""))
    }

    fn set_statistic(&mut self, statistic: Statistic) {
        let key = statistics_key(self.game.file());
        self.settings.set_string(&key, &statistic.to_text());
        self.settings.save();
    }

    /// Count the current game, if it was started, as won or lost. Called
    /// when it is left: a new game (but not a restart, which is the same
    /// game), another game type, or quitting.
    fn update_statistics(&mut self) {
        if self.game.state < GameState::Running {
            return;
        }
        let mut statistic = self.statistic();
        let seconds = (self.game.elapsed().as_secs_f64() + 0.5) as u32;
        statistic.add(self.game.state == GameState::Won, seconds);
        self.set_statistic(statistic);
    }

    fn title(&self) -> String {
        format!("{APP_NAME} — {}", self.game.name())
    }

    fn menus(&mut self) -> Vec<MenuData> {
        let started = self.game.state > GameState::Begin || self.game.can_undo();
        let mut options: Vec<MenuItemData> = self
            .game
            .options()
            .iter()
            .enumerate()
            .map(|(i, o)| MenuItemData {
                kind: if o.group.is_some() { MenuItemKind::Radio } else { MenuItemKind::Check },
                checked: o.value,
                ..item(&format!("option:{i}"), &o.label, "", true)
            })
            .collect();
        if options.is_empty() {
            options.push(item("", "No options", "", false));
        }
        let game = &self.game;
        let name = game.name().to_string();
        vec![
            menu(
                "Game",
                vec![
                    item("new-game", "New Game", "Ctrl+N", true),
                    item("restart", "Restart", "", started),
                    item("statistics", "Statistics", "", true),
                    item("select-game", "Select Game…", "Ctrl+O", true),
                    submenu("recent", "Recently Played", !self.recent.is_empty()),
                    separator(),
                    item("close", "Close", "Ctrl+W", true),
                ],
            ),
            menu(
                "View",
                vec![
                    check("fullscreen", "Fullscreen", "F11", self.fullscreen),
                    check("toggle-toolbar", "Toolbar", "", self.show_toolbar),
                    check("toggle-statusbar", "Statusbar", "", self.show_statusbar),
                    separator(),
                    submenu("card-style", "Card Style", true),
                ],
            ),
            menu(
                "Control",
                vec![
                    item("undo", "Undo Move", "Ctrl+Z", game.can_undo()),
                    item("redo", "Redo Move", "Shift+Ctrl+Z", game.can_redo()),
                    item("deal", "Deal", "Ctrl+D", game.can_deal()),
                    item("hint", "Hint", "Ctrl+H", game.state <= GameState::Running),
                    separator(),
                    check("toggle-click-to-move", "Click to Move", "", self.board.click_to_move),
                    check("toggle-sound", "Sound", "", self.sound),
                ],
            ),
            menu(&name, options),
            menu(
                "Help",
                vec![
                    item("help", "Contents", "F1", true),
                    item("help-game", &format!("Help on {name}"), "Shift+F1", true),
                    item("shortcuts", "Keyboard Shortcuts", "Ctrl+?", true),
                    separator(),
                    item("about", "About", "", true),
                ],
            ),
        ]
    }

    fn theme_items(&self) -> Vec<MenuItemData> {
        theme::theme_files()
            .map(|file| MenuItemData {
                kind: MenuItemKind::Radio,
                checked: file == self.theme.file_name,
                ..item(&format!("theme:{file}"), &theme::display_name(file), "", true)
            })
            .collect()
    }

    fn recent_items(&self) -> Vec<MenuItemData> {
        self.recent
            .iter()
            .map(|file| item(&format!("game:{file}"), &rules::display_name(file), "", true))
            .collect()
    }
}

/// Copy a pixmap into a Slint image. tiny-skia pixmaps are premultiplied.
fn to_slint_image(pixmap: &resvg::tiny_skia::Pixmap) -> slint::Image {
    let buffer =
        SharedPixelBuffer::<slint::Rgba8Pixel>::clone_from_slice(pixmap.data(), pixmap.width(), pixmap.height());
    slint::Image::from_rgba8_premultiplied(buffer)
}

/// A cheap fingerprint of every card on the table, to tell whether an input
/// event changed anything worth redrawing.
fn table_fingerprint(table: &game::Table) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    for slot in &table.slots {
        slot.cards.len().hash(&mut h);
        for c in &slot.cards {
            (c.rank, c.suit as u8, c.face_up).hash(&mut h);
        }
    }
    h.finish()
}

struct Ui {
    window: slint::Weak<AppWindow>,
    app: Rc<RefCell<App>>,
    resize_timer: Timer,
    autoplay_timer: Timer,
    long_press_timer: Timer,
    repaint_pending: std::cell::Cell<bool>,
    games: Vec<&'static str>,
    sound: Sound,
    log: InputLog,
}

/// Optional record of input events, written when started with --log-input,
/// to diagnose input problems (e.g. touch screens) on machines we can't
/// test on. Kept next to the settings file.
struct InputLog {
    file: RefCell<Option<std::fs::File>>,
    start: std::time::Instant,
}

impl InputLog {
    fn new() -> Self {
        InputLog { file: RefCell::new(None), start: std::time::Instant::now() }
    }

    fn path() -> Option<std::path::PathBuf> {
        settings::data_file_path("input-log.txt")
    }

    /// Start a fresh log; returns where it is written.
    fn start(&self) -> Option<std::path::PathBuf> {
        let path = Self::path()?;
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let file = std::fs::File::create(&path).ok()?;
        *self.file.borrow_mut() = Some(file);
        self.log(|| {
            format!(
                "{APP_NAME} {} on {} {}",
                env!("CARGO_PKG_VERSION"),
                std::env::consts::OS,
                std::env::consts::ARCH
            )
        });
        Some(path)
    }

    fn log(&self, message: impl FnOnce() -> String) {
        if let Some(file) = self.file.borrow_mut().as_mut() {
            use std::io::Write;
            let t = self.start.elapsed().as_secs_f32();
            let _ = writeln!(file, "{t:9.3}  {}", message());
            let _ = file.flush();
        }
    }
}

impl Ui {
    fn scale(&self) -> f32 {
        self.window.upgrade().map(|w| w.window().scale_factor()).unwrap_or(1.0)
    }

    /// Re-layout and repaint the board. With `rerender`, card images are
    /// regenerated if the card size changed.
    fn refresh_board(&self, rerender: bool) {
        let Some(window) = self.window.upgrade() else { return };
        let scale = window.window().scale_factor();
        let width = (window.get_board_width() * scale).round() as i32;
        let height = (window.get_board_height() * scale).round() as i32;
        if width <= 0 || height <= 0 {
            return;
        }
        let mut app = self.app.borrow_mut();
        let app = &mut *app;
        app.board.scale = scale;
        let theme = &app.theme;
        let (cw, ch) = app.board.setup_geometry(&app.game, width, height, |w, h, p| theme.card_size(w, h, p));
        let stale = app.images.as_ref().is_none_or(|i| i.width as i32 != cw || i.height as i32 != ch);
        if stale {
            if !rerender && app.images.is_some() {
                return;
            }
            app.images = Some(app.theme.render(cw as u32, ch as u32));
        }
        let images = app.images.as_ref().unwrap();
        // Draw straight into the buffer the window shows: at tablet
        // resolutions the board is over 20 MB, so copies are costly.
        let mut buffer = SharedPixelBuffer::<Rgba8Pixel>::new(width as u32, height as u32);
        app.board.paint_into(&app.game, images, buffer.make_mut_bytes());
        window.set_board_image(slint::Image::from_rgba8(buffer));

        // The dragged cards, if any, on their own layer.
        match app.board.drag_layer(images) {
            Some(layer) => {
                window.set_drag_width(layer.width() as f32 / scale);
                window.set_drag_height(layer.height() as f32 / scale);
                window.set_drag_image(to_slint_image(&layer));
                window.set_drag_visible(true);
                Self::place_drag_layer(&window, &app.board);
            }
            None => window.set_drag_visible(false),
        }
    }

    fn place_drag_layer(window: &AppWindow, board: &Board) {
        if let Some((x, y)) = board.drag_position() {
            window.set_drag_x(x as f32 / board.scale);
            window.set_drag_y(y as f32 / board.scale);
        }
    }

    /// Redraw the board once the current batch of input events is handled,
    /// however many of them asked for it.
    fn request_repaint(self: &Rc<Self>) {
        if self.repaint_pending.replace(true) {
            return;
        }
        let ui = self.clone();
        Timer::single_shot(Duration::ZERO, move || {
            ui.repaint_pending.set(false);
            ui.refresh_board(false);
        });
    }

    /// Push game state (status bar, menus, toolbar) to the window.
    fn sync(&self) {
        let Some(window) = self.window.upgrade() else { return };
        let mut app = self.app.borrow_mut();
        window.set_game_title(app.title().into());
        let show_score = !app.game.scores_disabled();
        let game = &app.game;
        window.set_status_message(game.table.status_message.as_str().into());
        window.set_score_text(game.table.score.as_str().into());
        window.set_show_score(show_score);
        window.set_can_undo(game.can_undo());
        window.set_can_redo(game.can_redo());
        window.set_can_deal(game.can_deal());
        window.set_can_restart(game.state > GameState::Begin || game.can_undo());
        window.set_show_toolbar(app.show_toolbar);
        window.set_show_statusbar(app.show_statusbar);
        window.set_fullscreen_mode(app.fullscreen);
        window.set_board_cursor(match app.board.cursor {
            Cursor::Default => BoardCursor::Default,
            Cursor::Open => BoardCursor::Open,
            Cursor::Closed => BoardCursor::Closed,
        });
        window.set_menus(ModelRc::new(VecModel::from(app.menus())));
        window.set_theme_items(ModelRc::new(VecModel::from(app.theme_items())));
        window.set_recent_items(ModelRc::new(VecModel::from(app.recent_items())));
        self.sync_clock(&window, &app.game);
    }

    fn sync_clock(&self, window: &AppWindow, game: &Game) {
        let secs = game.elapsed().as_secs();
        let text = format!("{:02}:{:02}:{:02}", secs / 3600, (secs / 60) % 60, secs % 60);
        if window.get_time_text() != text {
            window.set_time_text(text.into());
        }
    }

    fn show_dialog(&self, dialog: Dialog, message: &str, buttons: &[(&str, &str)]) {
        let Some(window) = self.window.upgrade() else { return };
        self.app.borrow_mut().dialog = Some(dialog);
        window.set_dialog_message(message.into());
        let buttons: Vec<DialogButton> =
            buttons.iter().map(|(id, label)| DialogButton { id: (*id).into(), label: (*label).into() }).collect();
        window.set_dialog_buttons(ModelRc::new(VecModel::from(buttons)));
        window.set_dialog_visible(true);
    }

    fn hide_dialog(&self) {
        self.app.borrow_mut().dialog = None;
        if let Some(window) = self.window.upgrade() {
            window.set_dialog_visible(false);
            window.set_chooser_visible(false);
            window.set_stats_visible(false);
            window.set_shortcuts_visible(false);
        }
    }

    /// After any move: run autoplay, show the end-of-game dialog once,
    /// refresh everything.
    fn after_move(self: &Rc<Self>, autoplay: bool) {
        if autoplay {
            let ui = self.clone();
            self.autoplay_timer.start(TimerMode::SingleShot, AUTOPLAY_DELAY, move || {
                let again = {
                    let mut app = ui.app.borrow_mut();
                    app.game.run_delayed_call();
                    app.game.has_delayed_call()
                };
                ui.after_move(again);
            });
        }
        let state = self.app.borrow().game.state;
        let show_end = state >= GameState::Over && !self.app.borrow().shown_end_dialog && !autoplay;
        if show_end {
            self.app.borrow_mut().shown_end_dialog = true;
            self.play(if state == GameState::Won { "victory" } else { "splat" });
            if state == GameState::Won {
                self.show_dialog(
                    Dialog::GameOver,
                    "Congratulations, you have won!",
                    &[("close", "Close"), ("new-game", "New Game")],
                );
            } else {
                self.show_dialog(
                    Dialog::GameOver,
                    "There are no more moves",
                    &[("undo", "Undo Move"), ("close", "Close"), ("restart", "Restart"), ("new-game", "New Game")],
                );
            }
        }
        self.refresh_board(false);
        self.sync();
    }

    fn play(&self, name: &'static str) {
        if self.app.borrow().sound {
            self.sound.play(name);
        }
    }

    fn stop_autoplay(&self) {
        self.autoplay_timer.stop();
        self.app.borrow_mut().game.cancel_delayed_call();
    }

    fn new_game(self: &Rc<Self>, restart: bool) {
        self.stop_autoplay();
        let result = {
            let mut app = self.app.borrow_mut();
            let app = &mut *app;
            app.board.abort(&mut app.game);
            app.board.reset_focus();
            app.shown_end_dialog = false;
            if !restart {
                app.update_statistics();
            }
            if restart { app.game.restart() } else { app.game.new_game(random_seed()) }
        };
        self.hide_dialog();
        if let Err(e) = result {
            self.show_dialog(Dialog::Error, &format!("This game could not be dealt.\n\n{e}"), &[("close", "Close")]);
        }
        // A new layout can change the card size.
        self.refresh_board(true);
        self.sync();
    }

    fn switch_game(self: &Rc<Self>, file: &str) {
        self.stop_autoplay();
        let loaded = {
            let app = self.app.borrow();
            App::load_game(&app.settings, file)
        };
        match loaded {
            Ok(game) => {
                let mut app = self.app.borrow_mut();
                let app = &mut *app;
                app.board.abort(&mut app.game);
                app.board.reset_focus();
                app.update_statistics();
                app.game = game;
                app.shown_end_dialog = false;
                app.remember_game();
            }
            Err(e) => {
                let name = rules::display_name(file);
                self.show_dialog(Dialog::Error, &format!("{name} could not be loaded.\n\n{e}"), &[("close", "Close")]);
            }
        }
        self.refresh_board(true);
        self.sync();
    }

    fn open_chooser(&self) {
        let Some(window) = self.window.upgrade() else { return };
        self.app.borrow_mut().dialog = Some(Dialog::Chooser);
        let current = self.app.borrow().game.file().to_string();
        let names: Vec<SharedString> = self.games.iter().map(|g| rules::display_name(g).into()).collect();
        window.set_chooser_names(ModelRc::new(VecModel::from(names)));
        window.set_chooser_selected(self.games.iter().position(|g| *g == current).unwrap_or(0) as i32);
        window.set_chooser_visible(true);
    }

    fn chooser_response(self: &Rc<Self>, select: bool, index: i32) {
        self.hide_dialog();
        if select {
            if let Some(file) = self.games.get(index as usize) {
                self.switch_game(file);
            }
        }
    }

    fn show_statistics(&self) {
        let Some(window) = self.window.upgrade() else { return };
        let mut app = self.app.borrow_mut();
        app.dialog = Some(Dialog::Statistics);
        let s = app.statistic();
        window.set_stats_game(app.game.name().into());
        window.set_stats_wins(s.wins.to_string().into());
        window.set_stats_total(s.total.to_string().into());
        let percentage = if s.total == 0 {
            "N/A".to_string()
        } else {
            format!("{}%", (100.0 * s.wins as f64 / s.total as f64 + 0.5) as u32)
        };
        window.set_stats_percentage(percentage.into());
        window.set_stats_best(stat_time(s.best).into());
        window.set_stats_worst(stat_time(s.worst).into());
        window.set_stats_visible(true);
    }

    fn statistics_response(&self, reset: bool) {
        if reset {
            self.app.borrow_mut().set_statistic(Statistic::default());
            self.show_statistics();
        } else {
            self.hide_dialog();
        }
    }

    fn open_help(&self, page: &str) {
        if let Err(e) = help::open(page) {
            self.show_dialog(Dialog::Error, &format!("The help could not be opened.\n\n{e}"), &[("close", "Close")]);
        }
    }

    fn action(self: &Rc<Self>, id: &str) {
        self.log.log(|| format!("action {id}"));
        if self.app.borrow().dialog.is_some() {
            return;
        }
        match id {
            "new-game" => self.new_game(false),
            "restart" => self.new_game(true),
            "select-game" => self.open_chooser(),
            "statistics" => self.show_statistics(),
            "help" => self.open_help(help::CONTENTS),
            "help-game" => {
                let file = self.app.borrow().game.file().to_string();
                self.open_help(&file);
            }
            "shortcuts" => {
                self.app.borrow_mut().dialog = Some(Dialog::Shortcuts);
                if let Some(window) = self.window.upgrade() {
                    window.set_shortcuts_visible(true);
                }
            }
            "close" => {
                let _ = slint::quit_event_loop();
            }
            "undo" | "redo" => {
                self.stop_autoplay();
                {
                    let mut app = self.app.borrow_mut();
                    let app = &mut *app;
                    app.board.abort(&mut app.game);
                    if id == "undo" {
                        app.game.undo();
                        app.shown_end_dialog = false;
                    } else {
                        app.game.redo();
                    }
                }
                self.after_move(false);
            }
            "deal" => {
                let autoplay = {
                    let mut app = self.app.borrow_mut();
                    let app = &mut *app;
                    if !app.game.can_deal() {
                        return;
                    }
                    app.board.abort(&mut app.game);
                    app.game.deal();
                    app.game.has_delayed_call()
                };
                self.after_move(autoplay);
            }
            "hint" => {
                let message = {
                    let mut app = self.app.borrow_mut();
                    // Getting a hint starts the game.
                    app.game.start();
                    if app.game.state != GameState::Running {
                        return;
                    }
                    app.game.hint()
                };
                self.sync();
                self.show_dialog(Dialog::Hint, &message, &[("ok", "OK")]);
            }
            "fullscreen" => {
                let fullscreen = {
                    let mut app = self.app.borrow_mut();
                    app.fullscreen = !app.fullscreen;
                    app.fullscreen
                };
                if let Some(window) = self.window.upgrade() {
                    window.window().set_fullscreen(fullscreen);
                }
                self.sync();
            }
            "toggle-toolbar" | "toggle-statusbar" | "toggle-click-to-move" | "toggle-sound" => {
                {
                    let mut app = self.app.borrow_mut();
                    let app = &mut *app;
                    let (key, value) = match id {
                        "toggle-toolbar" => {
                            app.show_toolbar = !app.show_toolbar;
                            ("show-toolbar", app.show_toolbar)
                        }
                        "toggle-statusbar" => {
                            app.show_statusbar = !app.show_statusbar;
                            ("show-statusbar", app.show_statusbar)
                        }
                        "toggle-sound" => {
                            app.sound = !app.sound;
                            ("sound", app.sound)
                        }
                        _ => {
                            app.board.abort(&mut app.game);
                            app.board.click_to_move = !app.board.click_to_move;
                            ("click-to-move", app.board.click_to_move)
                        }
                    };
                    app.settings.set_bool(key, value);
                    app.settings.save();
                }
                self.sync();
            }
            "about" => {
                self.show_dialog(
                    Dialog::About,
                    &format!(
                        "{APP_NAME} {}\n\n\
                         Copyright © 2026 The Rusty Solitaire authors\n\n\
                         Based on GNOME AisleRiot by Jonathan Blandford, Felix Bellaby,\n\
                         Rosanna Yuen, Callum McKenzie, Vincent Povirk and many others.\n\n\
                         Licensed under the GNU General Public License, version 3 or later.",
                        env!("CARGO_PKG_VERSION")
                    ),
                    &[("close", "Close")],
                );
            }
            _ if id.starts_with("theme:") => {
                let file = &id["theme:".len()..];
                if let Some(theme) = CardTheme::load(file) {
                    let mut app = self.app.borrow_mut();
                    app.theme = theme;
                    app.images = None;
                    app.settings.set_string("card-theme", file);
                    app.settings.save();
                }
                self.refresh_board(true);
                self.sync();
            }
            _ if id.starts_with("game:") => {
                let file = id["game:".len()..].to_string();
                if file != self.app.borrow().game.file() {
                    self.switch_game(&file);
                }
            }
            _ if id.starts_with("option:") => {
                let Ok(index) = id["option:".len()..].parse::<usize>() else { return };
                {
                    let mut app = self.app.borrow_mut();
                    let options = app.game.options();
                    let Some(option) = options.get(index) else { return };
                    // A radio item that is already on stays on.
                    if option.group.is_some() && option.value {
                        return;
                    }
                    let value = option.group.is_some() || !option.value;
                    app.game.set_option(index, value);
                    app.save_options();
                }
                // Changing the rules starts a new game.
                self.new_game(false);
            }
            _ => {}
        }
    }

    /// game_over_dialog_response_cb: on the game-over dialog, Close quits
    /// the program, and dismissing it (Escape) starts a new game. Other
    /// dialogs just close.
    fn dialog_response(self: &Rc<Self>, id: &str) {
        self.log.log(|| format!("dialog response {id}"));
        let dialog = self.app.borrow().dialog;
        self.hide_dialog();
        let game_over = dialog == Some(Dialog::GameOver);
        let id = match id {
            "default" | "dismiss" if game_over => "new-game",
            "close" if game_over => "quit",
            other => other,
        };
        match id {
            "new-game" | "restart" | "undo" => self.action(id),
            "quit" => {
                let _ = slint::quit_event_loop();
            }
            _ => {}
        }
    }

    fn board_event(self: &Rc<Self>, x: f32, y: f32, f: impl FnOnce(&mut App, i32, i32) -> board::Outcome) {
        if self.app.borrow().dialog.is_some() {
            return;
        }
        let scale = self.scale();
        let (px, py) = ((x * scale) as i32, (y * scale) as i32);
        let (outcome, changed) = {
            let mut app = self.app.borrow_mut();
            let before = (table_fingerprint(&app.game.table), app.board.scene_key(), app.game.state);
            let outcome = f(&mut app, px, py);
            let after = (table_fingerprint(&app.game.table), app.board.scene_key(), app.game.state);
            (outcome, before != after)
        };
        if let Some(name) = outcome.sound {
            self.play(name);
        }
        // Pressing a card to start a drag changes nothing visible: skip the
        // (costly, on large screens) redraw.
        if changed || outcome.autoplay {
            self.after_move(outcome.autoplay);
        }
    }
}

impl Ui {
    /// A key for the board (cursor keys, Space, Return, Ctrl+A); returns
    /// whether it was one.
    fn board_key(self: &Rc<Self>, name: &str, shift: bool, ctrl: bool) -> bool {
        let key = match name {
            "left" => Key::Left,
            "right" => Key::Right,
            "up" => Key::Up,
            "down" => Key::Down,
            "home" => Key::Home,
            "end" => Key::End,
            "page-up" => Key::PageUp,
            "page-down" => Key::PageDown,
            "space" => Key::Space,
            "activate" => Key::Activate,
            "select-all" => Key::SelectAll,
            "deselect-all" => Key::DeselectAll,
            _ => return false,
        };
        self.log.log(|| format!("key {name}{}{}", if shift { " +shift" } else { "" }, if ctrl { " +ctrl" } else { "" }));
        if self.app.borrow().dialog.is_some() {
            return true;
        }
        let (outcome, changed) = {
            let mut app = self.app.borrow_mut();
            let app = &mut *app;
            let before = (table_fingerprint(&app.game.table), app.board.scene_key(), app.game.state);
            let outcome = app.board.key(&mut app.game, key, shift, ctrl);
            let after = (table_fingerprint(&app.game.table), app.board.scene_key(), app.game.state);
            (outcome, before != after)
        };
        if let Some(name) = outcome.sound {
            self.play(name);
        }
        if changed || outcome.autoplay {
            self.after_move(outcome.autoplay);
        }
        true
    }
}

fn main() -> Result<(), slint::PlatformError> {
    let window = AppWindow::new()?;
    // Lets Linux desktops match the window to io.github.rustysolitaire.RustySolitaire.desktop
    // (its name and icon in the dock / task switcher). Needs the platform
    // that creating the window sets up, and must come before it is shown.
    // Only cosmetic, so a failure must not stop the game from starting.
    if let Err(e) = slint::set_xdg_app_id(APP_ID) {
        eprintln!("could not set the application id: {e}");
    }
    let app = Rc::new(RefCell::new(App::new(Settings::load())));
    let ui = Rc::new(Ui {
        window: window.as_weak(),
        app,
        resize_timer: Timer::default(),
        autoplay_timer: Timer::default(),
        long_press_timer: Timer::default(),
        repaint_pending: std::cell::Cell::new(false),
        games: rules::game_files(),
        sound: Sound::new(),
        log: InputLog::new(),
    });
    // To diagnose input problems on machines we can't test on.
    if std::env::args().any(|a| a == "--log-input") {
        if let Some(path) = ui.log.start() {
            eprintln!("logging input to {}", path.display());
            ui.log.log(|| format!("scale factor {}", window.window().scale_factor()));
        }
    }

    {
        let ui = ui.clone();
        window.on_action(move |id| ui.action(&id));
    }
    {
        let ui = ui.clone();
        window.on_dialog_response(move |id| ui.dialog_response(&id));
    }
    {
        let ui = ui.clone();
        window.on_chooser_response(move |select, index| ui.chooser_response(select, index));
    }
    {
        let ui = ui.clone();
        window.on_stats_response(move |reset| ui.statistics_response(reset));
    }
    {
        let ui = ui.clone();
        window.on_shortcuts_close(move || ui.hide_dialog());
    }
    {
        let ui = ui.clone();
        window.on_board_key(move |name, shift, ctrl| ui.board_key(&name, shift, ctrl));
    }
    {
        let ui_weak = Rc::downgrade(&ui);
        window.on_board_resized(move || {
            let Some(ui) = ui_weak.upgrade() else { return };
            // Quick repaint with the current card images, full re-render
            // once resizing settles.
            ui.refresh_board(false);
            let ui2 = ui.clone();
            ui.resize_timer.start(TimerMode::SingleShot, RESIZE_DEBOUNCE, move || ui2.refresh_board(true));
        });
    }
    {
        let ui = ui.clone();
        window.on_board_press(move |x, y, button, touch| {
            ui.log.log(|| format!("press   {} button {button} at {x:.0},{y:.0}", if touch { "finger" } else { "mouse " }));
            ui.board_event(x, y, |app, x, y| app.board.press(&mut app.game, x, y, button as u8, touch));
            if touch && button == 1 {
                // Resting a finger on a covered card shows it.
                let ui2 = ui.clone();
                ui.long_press_timer.start(TimerMode::SingleShot, LONG_PRESS, move || {
                    let shown = {
                        let mut app = ui2.app.borrow_mut();
                        let app = &mut *app;
                        app.board.long_press(&mut app.game)
                    };
                    if shown {
                        ui2.log.log(|| "long press".into());
                        ui2.refresh_board(false);
                    }
                });
            }
        });
    }
    {
        let ui = ui.clone();
        window.on_board_cancel(move || {
            ui.log.log(|| "cancel".into());
            ui.long_press_timer.stop();
            {
                let mut app = ui.app.borrow_mut();
                let app = &mut *app;
                app.board.cancel(&mut app.game);
            }
            ui.refresh_board(false);
            ui.sync();
        });
    }
    {
        let ui = ui.clone();
        window.on_board_release(move |x, y| {
            ui.log.log(|| format!("release at {x:.0},{y:.0}"));
            ui.long_press_timer.stop();
            ui.board_event(x, y, |app, x, y| app.board.release(&mut app.game, x, y));
        });
    }
    {
        let ui = ui.clone();
        window.on_board_motion(move |x, y| {
            ui.log.log(|| format!("move    to {x:.0},{y:.0}"));
            if ui.app.borrow().dialog.is_some() {
                return;
            }
            let scale = ui.scale();
            let scene_changed = {
                let mut app = ui.app.borrow_mut();
                let app = &mut *app;
                let before = app.board.scene_key();
                app.board.motion(&mut app.game, (x * scale) as i32, (y * scale) as i32);
                before != app.board.scene_key()
            };
            if scene_changed {
                // A drag started or the drop target changed.
                ui.request_repaint();
            } else if let Some(window) = ui.window.upgrade() {
                // Most moves just carry the dragged cards along.
                Ui::place_drag_layer(&window, &ui.app.borrow().board);
            }
            if let Some(window) = ui.window.upgrade() {
                let cursor = match ui.app.borrow().board.cursor {
                    Cursor::Default => BoardCursor::Default,
                    Cursor::Open => BoardCursor::Open,
                    Cursor::Closed => BoardCursor::Closed,
                };
                window.set_board_cursor(cursor);
            }
        });
    }

    // The status bar clock.
    let clock = Timer::default();
    {
        let ui = ui.clone();
        clock.start(TimerMode::Repeated, Duration::from_millis(500), move || {
            if let Some(window) = ui.window.upgrade() {
                ui.sync_clock(&window, &ui.app.borrow().game);
            }
        });
    }

    ui.sync();
    window.run()?;
    // Leaving counts the game in progress (aisleriot_game_finalize).
    ui.app.borrow_mut().update_statistics();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn statistics_count_like_the_original() {
        let mut s = Statistic::parse("");
        assert_eq!(s, Statistic::default());
        s.add(false, 100);
        s.add(true, 0);
        s.add(true, 250);
        s.add(true, 90);
        assert_eq!(s, Statistic { wins: 3, total: 4, best: 90, worst: 250 });
        assert_eq!(Statistic::parse(&s.to_text()), s);
        assert_eq!(stat_time(s.worst), "4:10");
        assert_eq!(stat_time(0), "N/A");
    }
}
