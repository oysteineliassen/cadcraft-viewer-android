//! Brukergrensesnittet: verktøylinje, tegningsflate med berøringsgester, lagliste, valg av
//! modell/layout, PDF-eksport og «Om»-skjerm. Alt på norsk, med store trykkflater.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, channel};
use std::time::{Duration, Instant};

use cadcraft_doc::Space;
use eframe::egui::{self, Align, Color32, Layout, RichText, Vec2};

use crate::document::{self, Document, Orientation, Paper, PdfChoice};
use crate::platform::{self, Event};
use crate::view::Viewer;

/// Ferdig PDF (filnavn og innhold) eller feilmelding.
type ExportResult = Result<(String, Vec<u8>), String>;

/// Skjermer som kan ligge over tegningen.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Panel {
    None,
    Layers,
    Spaces,
    Menu,
    Pdf,
    About,
}

enum State {
    Empty,
    Loading { name: String, rx: Receiver<Result<Document, String>>, since: Instant },
    Failed { name: String, error: String },
    Open(Box<Opened>),
}

struct Opened {
    doc: Document,
    space: usize,
    viewer: Viewer,
}

pub struct ViewerApp {
    state: State,
    panel: Panel,
    dark: bool,
    pdf: PdfChoice,
    exporting: Option<Receiver<ExportResult>>,
    toast: Option<(String, Instant)>,
    layer_filter: String,
}

impl ViewerApp {
    pub fn new(cc: &eframe::CreationContext<'_>, open: Option<(PathBuf, String)>) -> ViewerApp {
        platform::set_context(&cc.egui_ctx);
        let dark = load_setting("bakgrunn").map(|v| v != "lys").unwrap_or(true);
        style(&cc.egui_ctx, dark);
        let mut app = ViewerApp {
            state: State::Empty,
            panel: Panel::None,
            dark,
            pdf: PdfChoice::default(),
            exporting: None,
            toast: None,
            layer_filter: String::new(),
        };
        if let Some((path, name)) = open {
            app.open(&cc.egui_ctx, path, name);
        }
        app
    }

    fn open(&mut self, ctx: &egui::Context, path: PathBuf, name: String) {
        let (tx, rx) = channel();
        let ctx = ctx.clone();
        let shown = if name.is_empty() { path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default() } else { name.clone() };
        let spawned = std::thread::Builder::new()
            .name("load".into())
            // DWG-leseren kan bruke mye stakk på dypt nestede filer.
            .stack_size(256 << 20)
            .spawn(move || {
                let r = document::load(&path, &name);
                let _ = tx.send(r);
                ctx.request_repaint();
            });
        self.panel = Panel::None;
        self.state = match spawned {
            Ok(_) => State::Loading { name: shown, rx, since: Instant::now() },
            Err(e) => State::Failed { name: shown, error: format!("Kunne ikke starte lesing: {e}") },
        };
    }

    fn toast(&mut self, text: impl Into<String>) {
        self.toast = Some((text.into(), Instant::now()));
    }

    fn set_dark(&mut self, ctx: &egui::Context, dark: bool) {
        self.dark = dark;
        style(ctx, dark);
        save_setting("bakgrunn", if dark { "mørk" } else { "lys" });
    }

    fn poll(&mut self, ctx: &egui::Context) {
        for e in platform::take() {
            match e {
                Event::Open { path, name } => {
                    log::info!("åpner «{name}» ({})", path.display());
                    self.open(ctx, path, name);
                }
                Event::Message(m) => {
                    log::info!("melding: {m}");
                    self.toast(m);
                }
            }
        }
        if let State::Loading { rx, name, .. } = &self.state
            && let Ok(r) = rx.try_recv()
        {
            self.state = match r {
                Ok(doc) => {
                    log::info!(
                        "lest «{}»: {} objekter i modellrommet, {} lag, rom: {:?}",
                        doc.name,
                        doc.drawing.model.len(),
                        doc.drawing.layers.len(),
                        doc.spaces.iter().map(Document::space_label).collect::<Vec<_>>()
                    );
                    let mut viewer = Viewer::new(ctx);
                    viewer.rebuild(ctx, &doc.drawing, &doc.spaces[0], None);
                    let notes = doc.drawing.read_notes.len();
                    if notes > 0 {
                        log::info!("{notes} merknader fra leseren");
                    }
                    State::Open(Box::new(Opened { doc, space: 0, viewer }))
                }
                Err(error) => {
                    log::warn!("kunne ikke lese «{name}»: {error}");
                    State::Failed { name: name.clone(), error }
                }
            };
        }
        if let Some(rx) = &self.exporting
            && let Ok(r) = rx.try_recv()
        {
            self.exporting = None;
            match r {
                Ok((name, bytes)) => {
                    log::info!("PDF «{name}»: {} byte", bytes.len());
                    platform::save_pdf(name, bytes);
                }
                Err(e) => self.toast(e),
            }
        }
    }

    fn top_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            if ui.add(big_button("Åpne")).clicked() {
                platform::pick_file();
            }
            let State::Open(o) = &self.state else {
                return;
            };
            let has_spaces = o.doc.spaces.len() > 1;
            let space_label = Document::space_label(&o.doc.spaces[o.space]);
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui.add(big_button("Meny")).clicked() {
                    self.panel = toggle(self.panel, Panel::Menu);
                }
                if ui.add(big_button("Lag")).clicked() {
                    self.panel = toggle(self.panel, Panel::Layers);
                }
                if has_spaces && ui.add(big_button(short(&space_label, 16))).clicked() {
                    self.panel = toggle(self.panel, Panel::Spaces);
                }
                ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                    ui.add(egui::Label::new(RichText::new(&o.doc.name).strong()).truncate());
                });
            });
        });
    }

    fn canvas(&mut self, ui: &mut egui::Ui) {
        let rect = ui.available_rect_before_wrap();
        match &mut self.state {
            State::Empty => {
                ui.vertical_centered(|ui| {
                    ui.add_space((rect.height() * 0.25).max(20.0));
                    ui.heading("CAD-viser");
                    ui.add_space(8.0);
                    ui.label("Vis DWG- og DXF-tegninger. Gratis og uten reklame.");
                    ui.add_space(24.0);
                    if ui.add(big_button("Åpne tegning").min_size(Vec2::new(240.0, 64.0))).clicked() {
                        platform::pick_file();
                    }
                    ui.add_space(16.0);
                    ui.label(RichText::new("Du kan også åpne en .dwg- eller .dxf-fil fra filbehandleren, e-post eller Google Disk og velge «Åpne med» og CAD-viser.").weak());
                });
            }
            State::Loading { name, since, .. } => {
                ui.vertical_centered(|ui| {
                    ui.add_space((rect.height() * 0.3).max(20.0));
                    ui.add(egui::Spinner::new().size(56.0));
                    ui.add_space(16.0);
                    ui.label(format!("Leser «{name}» …"));
                    if since.elapsed() > Duration::from_secs(5) {
                        ui.label(RichText::new("Store tegninger kan ta litt tid.").weak());
                    }
                });
            }
            State::Failed { name, error } => {
                let mut again = false;
                ui.vertical_centered(|ui| {
                    ui.add_space((rect.height() * 0.15).max(20.0));
                    egui::Frame::new()
                        .fill(if ui.visuals().dark_mode { Color32::from_rgb(80, 30, 30) } else { Color32::from_rgb(255, 228, 228) })
                        .corner_radius(8.0)
                        .inner_margin(20.0)
                        .show(ui, |ui| {
                            ui.set_max_width(560.0);
                            ui.label(RichText::new("Kunne ikke åpne tegningen").heading().color(Color32::from_rgb(230, 80, 80)));
                            ui.add_space(6.0);
                            ui.label(RichText::new(name.as_str()).strong());
                            ui.add_space(6.0);
                            ui.label(error.as_str());
                        });
                    ui.add_space(20.0);
                    again = ui.add(big_button("Åpne en annen fil")).clicked();
                });
                if again {
                    platform::pick_file();
                }
            }
            State::Open(o) => {
                let resp = ui.allocate_rect(rect, egui::Sense::click_and_drag());
                let space = o.doc.spaces[o.space].clone();
                gestures(ui, &resp, rect, &mut o.viewer);
                o.viewer.show(ui, rect, self.dark, &o.doc.drawing, &space);
                if !o.viewer.has_image() || o.viewer.building_since.is_some_and(|t| t.elapsed() > Duration::from_millis(400)) {
                    let at = egui::pos2(rect.right() - 36.0, rect.bottom() - 36.0);
                    ui.put(egui::Rect::from_center_size(at, Vec2::splat(40.0)), egui::Spinner::new().size(32.0));
                }
                if o.viewer.list().is_some_and(|l| l.prims.is_empty()) && !o.viewer.is_building() {
                    ui.put(
                        egui::Rect::from_center_size(rect.center(), Vec2::new(rect.width().min(400.0), 80.0)),
                        egui::Label::new(RichText::new("Ingenting å vise her. Er alle lag slått av, eller er dette rommet tomt?").weak()),
                    );
                }
            }
        }
    }

    fn panels(&mut self, ctx: &egui::Context) {
        match self.panel {
            Panel::None => {}
            Panel::Layers => self.layers_panel(ctx),
            Panel::Spaces => self.spaces_panel(ctx),
            Panel::Menu => self.menu_panel(ctx),
            Panel::Pdf => self.pdf_panel(ctx),
            Panel::About => self.about_panel(ctx),
        }
    }

    fn layers_panel(&mut self, ctx: &egui::Context) {
        let State::Open(o) = &mut self.state else {
            self.panel = Panel::None;
            return;
        };
        let mut close = false;
        let mut changed = false;
        sheet(ctx, "Lag", &mut close, |ui| {
            ui.horizontal(|ui| {
                if ui.add(big_button("Alle på")).clicked() {
                    for l in &mut Arc::make_mut(&mut o.doc.drawing).layers {
                        changed |= !l.on;
                        l.on = true;
                    }
                }
                if ui.add(big_button("Alle av")).clicked() {
                    for l in &mut Arc::make_mut(&mut o.doc.drawing).layers {
                        changed |= l.on;
                        l.on = false;
                    }
                }
            });
            // Søk bare på desktop: NativeActivity viser ikke skjermtastaturet av seg selv.
            if cfg!(not(target_os = "android")) && o.doc.drawing.layers.len() > 12 {
                ui.add(egui::TextEdit::singleline(&mut self.layer_filter).hint_text("Søk etter lag").desired_width(f32::INFINITY));
            }
            ui.separator();
            let filter = self.layer_filter.to_lowercase();
            let mut names: Vec<(String, bool, bool, Color32)> = o
                .doc
                .drawing
                .layers
                .iter()
                .filter(|l| filter.is_empty() || l.name.to_lowercase().contains(&filter))
                .map(|l| {
                    let rgb = l.color.resolve(l.color, cadcraft_color::Color::Index(7));
                    let rgb = cadcraft_color::display_rgb(rgb, l.color == cadcraft_color::Color::Index(7), crate::view::background(ui.visuals().dark_mode));
                    (l.name.clone(), l.on, l.frozen, Color32::from_rgb(rgb.0, rgb.1, rgb.2))
                })
                .collect();
            names.sort_by_key(|n| n.0.to_lowercase());
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                for (name, on, frozen, color) in names {
                    let row = ui.horizontal(|ui| {
                        ui.set_min_height(48.0);
                        let (r, _) = ui.allocate_exact_size(Vec2::new(20.0, 20.0), egui::Sense::hover());
                        ui.painter().rect_filled(r, 3.0, color);
                        let mut v = on;
                        let text = if frozen { format!("{name} (frosset)") } else { name.clone() };
                        let resp = ui.add(egui::Checkbox::new(&mut v, text));
                        resp.clicked()
                    });
                    if row.inner
                        && let Some(l) = Arc::make_mut(&mut o.doc.drawing).layer_mut(&name)
                    {
                        l.on = !l.on;
                        changed = true;
                    }
                }
            });
        });
        if changed {
            let space = o.doc.spaces[o.space].clone();
            o.viewer.rebuild(ctx, &o.doc.drawing, &space, None);
        }
        if close {
            self.panel = Panel::None;
        }
    }

    fn spaces_panel(&mut self, ctx: &egui::Context) {
        let State::Open(o) = &mut self.state else {
            self.panel = Panel::None;
            return;
        };
        let mut close = false;
        let mut pick = None;
        sheet(ctx, "Modell og layouter", &mut close, |ui| {
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                for (i, s) in o.doc.spaces.iter().enumerate() {
                    let label = Document::space_label(s);
                    let text = if matches!(s, Space::Model) { format!("{label} (modellrom)") } else { label };
                    if ui.add(egui::Button::selectable(i == o.space, RichText::new(text).size(20.0)).min_size(Vec2::new(ui.available_width(), 52.0))).clicked() {
                        pick = Some(i);
                    }
                }
            });
        });
        if let Some(i) = pick {
            if i != o.space {
                o.space = i;
                o.viewer = Viewer::new(ctx);
                o.viewer.rebuild(ctx, &o.doc.drawing, &o.doc.spaces[i], None);
            }
            close = true;
        }
        if close {
            self.panel = Panel::None;
        }
    }

    fn menu_panel(&mut self, ctx: &egui::Context) {
        let mut close = false;
        let mut next = None;
        let mut dark = self.dark;
        let open = matches!(self.state, State::Open(_));
        sheet(ctx, "Meny", &mut close, |ui| {
            let w = ui.available_width();
            if open && ui.add(big_button("Vis hele tegningen").min_size(Vec2::new(w, 52.0))).clicked() {
                next = Some(Panel::None);
                if let State::Open(o) = &mut self.state {
                    o.viewer.fit();
                }
            }
            ui.add_space(4.0);
            ui.label("Bakgrunn");
            ui.horizontal(|ui| {
                ui.selectable_value(&mut dark, true, RichText::new("Mørk").size(20.0));
                ui.selectable_value(&mut dark, false, RichText::new("Lys").size(20.0));
            });
            ui.add_space(8.0);
            if open && ui.add(big_button("Eksporter til PDF").min_size(Vec2::new(w, 52.0))).clicked() {
                next = Some(Panel::Pdf);
            }
            if ui.add(big_button("Om appen og lisenser").min_size(Vec2::new(w, 52.0))).clicked() {
                next = Some(Panel::About);
            }
        });
        if dark != self.dark {
            self.set_dark(ctx, dark);
        }
        if let Some(p) = next {
            self.panel = p;
        } else if close {
            self.panel = Panel::None;
        }
    }

    fn pdf_panel(&mut self, ctx: &egui::Context) {
        let State::Open(o) = &self.state else {
            self.panel = Panel::None;
            return;
        };
        let model = matches!(o.doc.spaces[o.space], Space::Model);
        if model && self.pdf.paper == Paper::Layout {
            self.pdf.paper = Paper::A4;
        }
        let mut close = false;
        let mut go = false;
        let busy = self.exporting.is_some();
        let c = &mut self.pdf;
        sheet(ctx, "Eksporter til PDF", &mut close, |ui| {
            ui.label(format!("Rom: {}", Document::space_label(&o.doc.spaces[o.space])));
            ui.add_space(6.0);
            ui.label("Papir");
            ui.horizontal_wrapped(|ui| {
                for p in Paper::ALL {
                    if p == Paper::Layout && model {
                        continue;
                    }
                    ui.selectable_value(&mut c.paper, p, RichText::new(p.label()).size(20.0));
                }
            });
            if !(c.paper == Paper::Layout && !model) {
                ui.add_space(6.0);
                ui.label("Retning");
                ui.horizontal_wrapped(|ui| {
                    ui.selectable_value(&mut c.orientation, Orientation::Auto, RichText::new("Automatisk").size(20.0));
                    ui.selectable_value(&mut c.orientation, Orientation::Landscape, RichText::new("Liggende").size(20.0));
                    ui.selectable_value(&mut c.orientation, Orientation::Portrait, RichText::new("Stående").size(20.0));
                });
            }
            if model {
                ui.add_space(6.0);
                ui.label("Område");
                ui.horizontal_wrapped(|ui| {
                    ui.selectable_value(&mut c.only_visible, false, RichText::new("Hele tegningen").size(20.0));
                    ui.selectable_value(&mut c.only_visible, true, RichText::new("Det som vises nå").size(20.0));
                });
            }
            ui.add_space(6.0);
            ui.checkbox(&mut c.lineweights, "Bruk linjetykkelser");
            ui.label(RichText::new("Lag som er slått av, kommer ikke med.").weak());
            ui.add_space(12.0);
            if busy {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label("Lager PDF …");
                });
            } else if ui.add(big_button("Lag PDF").min_size(Vec2::new(ui.available_width(), 56.0))).clicked() {
                go = true;
            }
        });
        if go {
            self.start_export(ctx);
        }
        if close {
            self.panel = Panel::None;
        }
    }

    fn start_export(&mut self, ctx: &egui::Context) {
        let State::Open(o) = &self.state else { return };
        let Some(list) = o.viewer.list() else {
            self.toast("Vent til tegningen er ferdig tegnet.");
            return;
        };
        let space = o.doc.spaces[o.space].clone();
        let extents = list.bounds;
        let rect = ctx.content_rect();
        let window = o.viewer.cam.world_bounds(o.viewer_rect().unwrap_or(rect));
        let d = o.doc.drawing.clone();
        let choice = self.pdf.clone();
        let stem = std::path::Path::new(&o.doc.name).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "tegning".into());
        let name = match &space {
            Space::Model => format!("{stem}.pdf"),
            Space::Paper(l) => format!("{stem} - {l}.pdf"),
        };
        let title = stem.clone();
        let (tx, rx) = channel();
        let ctx2 = ctx.clone();
        let spawned = std::thread::Builder::new().name("pdf".into()).stack_size(64 << 20).spawn(move || {
            let r = document::export_pdf(&d, &space, &choice, &title, extents, window).map(|b| (name, b));
            let _ = tx.send(r);
            ctx2.request_repaint();
        });
        if spawned.is_ok() {
            self.exporting = Some(rx);
        }
    }

    fn about_panel(&mut self, ctx: &egui::Context) {
        let mut close = false;
        sheet(ctx, "Om CAD-viser", &mut close, |ui| {
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                crate::about::show(ui);
            });
        });
        if close {
            self.panel = Panel::None;
        }
    }

    /// Tilbake-knappen (Android) / Escape: lukk det som er åpent, ellers legg appen bort.
    fn back(&mut self, ctx: &egui::Context) {
        let pressed = ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::BrowserBack) || i.consume_key(egui::Modifiers::NONE, egui::Key::Escape));
        if !pressed {
            return;
        }
        if self.panel != Panel::None {
            self.panel = Panel::None;
        } else {
            platform::leave();
        }
    }
}

impl Opened {
    fn viewer_rect(&self) -> Option<egui::Rect> {
        self.viewer.last_rect()
    }
}

impl eframe::App for ViewerApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.poll(&ctx);
        self.back(&ctx);
        // Filer dratt inn i vinduet (desktop).
        let dropped = ctx.input(|i| i.raw.dropped_files.first().map(|f| f.path().to_path_buf()).filter(|p| !p.as_os_str().is_empty()));
        if let Some(path) = dropped {
            let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            self.open(&ctx, path, name);
        }

        let ppp = ctx.pixels_per_point();
        let [l, t, r, b] = platform::insets().map(|v| v / ppp);
        let bar_fill = ctx.global_style().visuals.panel_fill;
        egui::Panel::top("verktøy")
            .frame(egui::Frame::new().fill(bar_fill).inner_margin(egui::Margin { left: (8.0 + l) as i8, right: (8.0 + r) as i8, top: (6.0 + t).min(127.0) as i8, bottom: 6 }))
            .show(ui, |ui| self.top_bar(ui));
        if b > 0.0 {
            egui::Panel::bottom("bunnkant").exact_size(b).frame(egui::Frame::new().fill(bar_fill)).show(ui, |_| {});
        }
        egui::CentralPanel::default().frame(egui::Frame::new().fill(ctx.global_style().visuals.extreme_bg_color)).show(ui, |ui| self.canvas(ui));
        self.panels(&ctx);

        if let Some((text, at)) = &self.toast {
            let age = at.elapsed();
            if age > Duration::from_secs(4) {
                self.toast = None;
            } else {
                egui::Area::new(egui::Id::unique("melding")).anchor(egui::Align2::CENTER_BOTTOM, Vec2::new(0.0, -24.0 - b)).show(&ctx, |ui| {
                    egui::Frame::popup(ui.style()).show(ui, |ui| {
                        ui.set_max_width(480.0);
                        ui.label(text.as_str());
                    });
                });
                ctx.request_repaint_after(Duration::from_secs(4) - age);
            }
        }
    }
}

fn toggle(cur: Panel, p: Panel) -> Panel {
    if cur == p { Panel::None } else { p }
}

fn short(s: &str, n: usize) -> String {
    if s.chars().count() <= n { s.to_owned() } else { format!("{}…", s.chars().take(n - 1).collect::<String>()) }
}

fn big_button(text: impl Into<String>) -> egui::Button<'static> {
    egui::Button::new(RichText::new(text.into()).size(20.0)).min_size(Vec2::new(52.0, 48.0))
}

/// Et panel over tegningen med tittel og «Lukk»-knapp. På smale skjermer fyller det bredden.
fn sheet(ctx: &egui::Context, title: &str, close: &mut bool, add: impl FnOnce(&mut egui::Ui)) {
    let screen = ctx.content_rect();
    let ppp = ctx.pixels_per_point();
    let [_, top, _, bottom] = platform::insets().map(|v| v / ppp);
    let width = (screen.width() - 16.0).min(480.0);
    let height = (screen.height() - top - bottom - 80.0).max(200.0);
    egui::Window::new(title)
        .id(egui::Id::unique(("panel", title)))
        .title_bar(false)
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::RIGHT_TOP, Vec2::new(-8.0, top + 64.0))
        .fixed_size(Vec2::new(width, height))
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.heading(title);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.add(big_button("Lukk")).clicked() {
                        *close = true;
                    }
                });
            });
            ui.separator();
            add(ui);
        });
}

/// Knip for zoom, dra med én finger for å panorere, dobbelttrykk for hele tegningen. På desktop
/// zoomer musehjulet.
fn gestures(ui: &egui::Ui, resp: &egui::Response, rect: egui::Rect, v: &mut Viewer) {
    let (multi, scroll, zoom, hover) = ui.input(|i| (i.multi_touch(), i.smooth_scroll_delta, i.zoom_delta(), i.pointer.hover_pos()));
    let mut moved = false;
    if let Some(mt) = multi {
        if resp.contains_pointer() || rect.contains(mt.center_pos) {
            v.cam.pan(mt.translation_delta);
            v.cam.zoom_about(rect, mt.center_pos, f64::from(mt.zoom_delta));
            moved = true;
        }
    } else {
        if resp.dragged() {
            v.cam.pan(resp.drag_delta());
            moved = true;
        }
        if let Some(p) = hover.filter(|p| rect.contains(*p)) {
            let mut z = f64::from(zoom);
            if scroll.y != 0.0 {
                z *= (f64::from(scroll.y) * 0.004).exp();
            }
            if (z - 1.0).abs() > 1e-6 {
                v.cam.zoom_about(rect, p, z);
                moved = true;
            }
        }
    }
    if resp.double_clicked() {
        v.fit();
    } else if moved {
        v.touched();
    }
}

fn style(ctx: &egui::Context, dark: bool) {
    platform::set_light_bars(!dark);
    ctx.set_visuals(if dark { egui::Visuals::dark() } else { egui::Visuals::light() });
    ctx.global_style_mut(|s| {
        use egui::{FontId, TextStyle};
        s.text_styles.insert(TextStyle::Body, FontId::proportional(18.0));
        s.text_styles.insert(TextStyle::Button, FontId::proportional(20.0));
        s.text_styles.insert(TextStyle::Heading, FontId::proportional(24.0));
        s.text_styles.insert(TextStyle::Small, FontId::proportional(14.0));
        s.text_styles.insert(TextStyle::Monospace, FontId::monospace(14.0));
        s.spacing.interact_size = Vec2::new(48.0, 40.0);
        s.spacing.icon_width = 28.0;
        s.spacing.icon_width_inner = 16.0;
        s.spacing.item_spacing = Vec2::new(10.0, 10.0);
        s.spacing.button_padding = Vec2::new(14.0, 8.0);
        s.spacing.scroll.bar_width = 12.0;
        s.interaction.tooltip_delay = 0.8;
    });
}

fn settings_file() -> Option<PathBuf> {
    platform::data_dir().map(|d| d.join("innstillinger.txt"))
}

fn load_setting(key: &str) -> Option<String> {
    let text = std::fs::read_to_string(settings_file()?).ok()?;
    text.lines().find_map(|l| l.strip_prefix(key).and_then(|r| r.strip_prefix('=')).map(str::to_owned))
}

fn save_setting(key: &str, value: &str) {
    let Some(path) = settings_file() else { return };
    let text = std::fs::read_to_string(&path).unwrap_or_default();
    let mut lines: Vec<String> = text.lines().filter(|l| !l.starts_with(&format!("{key}="))).map(str::to_owned).collect();
    lines.push(format!("{key}={value}"));
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(path, lines.join("\n"));
}
