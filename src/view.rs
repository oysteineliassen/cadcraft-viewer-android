//! Visningsmotor: bygger CADCraft-visningslisten og rasteriserer den med tiny-skia i
//! bakgrunnstråder. UI-tråden viser siste ferdige bilde som en tekstur, flyttet og skalert til
//! gjeldende visning, så knip og dra er like jevne uansett hvor stor tegningen er. Når gesten har
//! stoppet, tegnes et skarpt bilde for den nye visningen.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::Arc;
use std::time::{Duration, Instant};

use cadcraft_color::Rgb;
use cadcraft_doc::{Drawing, Space};
use cadcraft_geom::{Bounds2, Vec2};
use cadcraft_render::DisplayList;
use cadcraft_render::raster::{RasterOptions, View};
use eframe::egui;

/// Største sidelengde (piksler) på et rasterisert bilde.
const MAX_SIDE: f32 = 4096.0;
/// Største avvik (piksler) mellom en bue og linjestykkene den tegnes med.
const TOLERANCE_PX: f64 = 0.2;
/// Visningslisten bygges på nytt når man har zoomet så mye lenger inn enn den ble laget for.
const REBUILD_AT: f64 = 1.6;
/// Hvor lenge visningen må stå i ro før et skarpt bilde tegnes.
const SETTLE: Duration = Duration::from_millis(120);

/// Kamera i skjermpunkter (egui-punkter) per tegningsenhet.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Camera {
    pub center: Vec2,
    /// Punkter per tegningsenhet.
    pub scale: f64,
}

impl Camera {
    pub fn fit(b: &Bounds2, size: egui::Vec2) -> Camera {
        if b.is_empty() || size.x <= 0.0 || size.y <= 0.0 {
            return Camera { center: Vec2::ZERO, scale: 1.0 };
        }
        let w = b.width().max(1e-9);
        let h = b.height().max(1e-9);
        let s = (f64::from(size.x) / w).min(f64::from(size.y) / h) * 0.95;
        Camera { center: b.center(), scale: if s.is_finite() && s > 0.0 { s } else { 1.0 } }
    }

    pub fn to_screen(self, rect: egui::Rect, p: Vec2) -> egui::Pos2 {
        let c = rect.center();
        egui::pos2(c.x + ((p.x - self.center.x) * self.scale) as f32, c.y - ((p.y - self.center.y) * self.scale) as f32)
    }

    pub fn to_world(self, rect: egui::Rect, p: egui::Pos2) -> Vec2 {
        let c = rect.center();
        Vec2::new(self.center.x + f64::from(p.x - c.x) / self.scale, self.center.y - f64::from(p.y - c.y) / self.scale)
    }

    /// Zoom med faktor `z` rundt skjermpunktet `about`.
    pub fn zoom_about(&mut self, rect: egui::Rect, about: egui::Pos2, z: f64) {
        if !(z.is_finite() && z > 0.0) {
            return;
        }
        let w = self.to_world(rect, about);
        let new_scale = (self.scale * z).clamp(1e-12, 1e12);
        let c = rect.center();
        self.scale = new_scale;
        self.center = Vec2::new(w.x - f64::from(about.x - c.x) / new_scale, w.y + f64::from(about.y - c.y) / new_scale);
    }

    pub fn pan(&mut self, delta: egui::Vec2) {
        self.center = Vec2::new(self.center.x - f64::from(delta.x) / self.scale, self.center.y + f64::from(delta.y) / self.scale);
    }

    /// Synlig område i tegningsenheter.
    pub fn world_bounds(&self, rect: egui::Rect) -> Bounds2 {
        let a = self.to_world(rect, rect.left_bottom());
        let b = self.to_world(rect, rect.right_top());
        Bounds2::new(a, b)
    }
}

/// Bakgrunn: mørk (som CADCraft) eller lys.
pub fn background(dark: bool) -> Rgb {
    if dark { RasterOptions::DARK_BACKGROUND } else { Rgb(255, 255, 255) }
}

/// Et ferdig rasterisert bilde og visningen det ble tegnet for.
struct Shot {
    texture: egui::TextureHandle,
    /// Verdensområdet bildet dekker.
    world: Bounds2,
}

struct ListJob {
    id: u64,
    cancel: Arc<AtomicBool>,
}

struct RasterRequest {
    id: u64,
    list: Arc<DisplayList>,
    view: View,
    background: Rgb,
}

struct RasterResult {
    id: u64,
    image: egui::ColorImage,
    world: Bounds2,
}

/// Det som skal tegnes ble sist rasterisert med disse innstillingene.
#[derive(Clone, Copy, PartialEq)]
struct Key {
    cam: Camera,
    size: [u32; 2],
    list_id: u64,
    dark: bool,
}

/// Visning av én tegning i ett rom (modell eller layout).
pub struct Viewer {
    pub cam: Camera,
    /// Gjeldende visningsliste og målestokken den ble bygget for.
    list: Option<(Arc<DisplayList>, f64, u64)>,
    building: Option<ListJob>,
    list_rx: Receiver<(u64, f64, DisplayList)>,
    list_tx: Sender<(u64, f64, DisplayList)>,
    raster_tx: Sender<RasterRequest>,
    result_rx: Receiver<RasterResult>,
    shot: Option<Shot>,
    rendered: Option<Key>,
    /// Bildet som er bestilt: innstillinger og løpenummer.
    pending: Option<(Key, u64)>,
    raster_seq: u64,
    next_id: u64,
    last_change: Instant,
    /// Tilpass til tegningen når første visningsliste er klar.
    fit_pending: bool,
    last_rect: Option<egui::Rect>,
    pub building_since: Option<Instant>,
}

impl Viewer {
    pub fn new(ctx: &egui::Context) -> Viewer {
        let (list_tx, list_rx) = channel();
        let (raster_tx, raster_rx) = channel::<RasterRequest>();
        let (result_tx, result_rx) = channel();
        let ctx2 = ctx.clone();
        // Én rasteriseringstråd; den hopper over forespørsler som allerede er utdatert.
        std::thread::Builder::new()
            .name("raster".into())
            .spawn(move || raster_worker(raster_rx, result_tx, ctx2))
            .ok();
        Viewer {
            cam: Camera { center: Vec2::ZERO, scale: 1.0 },
            list: None,
            building: None,
            list_rx,
            list_tx,
            raster_tx,
            result_rx,
            shot: None,
            rendered: None,
            pending: None,
            raster_seq: 0,
            next_id: 1,
            last_change: Instant::now(),
            fit_pending: true,
            last_rect: None,
            building_since: None,
        }
    }

    /// Start (på nytt) bygging av visningslisten, f.eks. etter at et lag er slått av/på.
    pub fn rebuild(&mut self, ctx: &egui::Context, d: &Arc<Drawing>, space: &Space, scale: Option<f64>) {
        if let Some(job) = self.building.take() {
            job.cancel.store(true, Ordering::Relaxed);
        }
        let id = self.next_id;
        self.next_id += 1;
        let cancel = Arc::new(AtomicBool::new(false));
        self.building = Some(ListJob { id, cancel: cancel.clone() });
        self.building_since.get_or_insert_with(Instant::now);
        let d = d.clone();
        let space = space.clone();
        let tx = self.list_tx.clone();
        let ctx = ctx.clone();
        // Uten kjent målestokk: bygg først grovt for å finne utstrekningen, så detaljert for den.
        let scale = scale.or(self.list.as_ref().map(|l| l.1));
        std::thread::Builder::new()
            .name("display-list".into())
            .stack_size(64 << 20)
            .spawn(move || {
                let build = |px_per_unit: f64| {
                    let opts = cadcraft_render::Options {
                        tolerance: TOLERANCE_PX / px_per_unit,
                        min_dash: 2.0 / px_per_unit,
                        transparency: true,
                        cancel: Some(cancel.clone()),
                        ..Default::default()
                    };
                    cadcraft_render::build(&d, &space, &opts)
                };
                let (list, used) = match scale {
                    Some(s) => (build(s), s),
                    None => {
                        let coarse = build(1e-3);
                        let b = sheet_or_bounds(&coarse);
                        // Detalj tilpasset et skjermbilde på ca. 2000 piksler.
                        let s = if b.is_empty() { 1.0 } else { 2000.0 / b.width().max(b.height()).max(1e-9) };
                        (build(s), s)
                    }
                };
                if !cancel.load(Ordering::Relaxed) {
                    let _ = tx.send((id, used, list));
                    ctx.request_repaint();
                }
            })
            .ok();
    }

    pub fn is_building(&self) -> bool {
        self.building.is_some()
    }

    pub fn has_image(&self) -> bool {
        self.shot.is_some()
    }

    pub fn last_rect(&self) -> Option<egui::Rect> {
        self.last_rect
    }

    pub fn list(&self) -> Option<&DisplayList> {
        self.list.as_ref().map(|l| &*l.0)
    }

    pub fn fit(&mut self) {
        self.fit_pending = true;
        self.last_change = Instant::now() - SETTLE;
    }

    pub fn touched(&mut self) {
        self.last_change = Instant::now();
    }

    /// Tegn visningen i `rect`, ta imot ferdige jobber og be om nye ved behov.
    pub fn show(&mut self, ui: &mut egui::Ui, rect: egui::Rect, dark: bool, d: &Arc<Drawing>, space: &Space) {
        let ctx = ui.ctx().clone();
        self.last_rect = Some(rect);
        // Ny visningsliste ferdig?
        while let Ok((id, scale, list)) = self.list_rx.try_recv() {
            if self.building.as_ref().is_some_and(|j| j.id == id) {
                self.building = None;
                self.building_since = None;
                self.list = Some((Arc::new(list), scale, id));
            }
        }
        if self.fit_pending
            && let Some(list) = self.list()
        {
            self.cam = Camera::fit(&sheet_or_bounds(list), rect.size());
            self.fit_pending = false;
        }
        // Ferdig bilde?
        while let Ok(r) = self.result_rx.try_recv() {
            if let Some((key, seq)) = self.pending
                && seq == r.id
            {
                let texture = ctx.load_texture("drawing", r.image, egui::TextureOptions::LINEAR);
                self.shot = Some(Shot { texture, world: r.world });
                self.rendered = Some(key);
                self.pending = None;
            }
        }

        let painter = ui.painter_at(rect);
        let bg = background(dark);
        painter.rect_filled(rect, 0.0, egui::Color32::from_rgb(bg.0, bg.1, bg.2));
        if let Some(shot) = &self.shot {
            let a = self.cam.to_screen(rect, Vec2::new(shot.world.min.x, shot.world.max.y));
            let b = self.cam.to_screen(rect, Vec2::new(shot.world.max.x, shot.world.min.y));
            let uv = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
            painter.image(shot.texture.id(), egui::Rect::from_min_max(a, b), uv, egui::Color32::WHITE);
        }
        if let Some(list) = self.list()
            && let Some(sheet) = &list.sheet
        {
            // Papirarket i en layout: tynn ramme rundt arket.
            let b = sheet.bounds();
            let a = self.cam.to_screen(rect, Vec2::new(b.min.x, b.max.y));
            let c = self.cam.to_screen(rect, Vec2::new(b.max.x, b.min.y));
            let col = if dark { egui::Color32::from_gray(150) } else { egui::Color32::from_gray(90) };
            painter.rect_stroke(egui::Rect::from_min_max(a, c), 0.0, egui::Stroke::new(1.0, col), egui::StrokeKind::Middle);
        }

        // Zoomet langt inn i forhold til detaljnivået: bygg listen på nytt så buer blir glatte.
        if let Some((_, built, _)) = &self.list
            && self.building.is_none()
        {
            let px = self.cam.scale * f64::from(ctx.pixels_per_point());
            if px > built * REBUILD_AT {
                let target = px * REBUILD_AT;
                self.rebuild(&ctx, d, space, Some(target));
            }
        }

        // Be om et skarpt bilde når visningen har stått i ro litt.
        let Some((list, _, list_id)) = self.list.clone() else { return };
        let ppp = ctx.pixels_per_point();
        let mut w = rect.width() * ppp;
        let mut h = rect.height() * ppp;
        let shrink = (MAX_SIDE / w.max(h)).min(1.0);
        w *= shrink;
        h *= shrink;
        let key = Key { cam: self.cam, size: [w.round().max(1.0) as u32, h.round().max(1.0) as u32], list_id, dark };
        let up_to_date = self.rendered == Some(key) || self.pending.is_some_and(|(k, _)| k == key);
        if !up_to_date {
            let idle = self.last_change.elapsed();
            if idle >= SETTLE || self.shot.is_none() {
                self.raster_seq += 1;
                let view = View { center: self.cam.center, scale: self.cam.scale * f64::from(ppp * shrink), width: key.size[0], height: key.size[1] };
                let _ = self.raster_tx.send(RasterRequest {
                    id: self.raster_seq,
                    list,
                    view,
                    background: bg,
                });
                self.pending = Some((key, self.raster_seq));
            } else {
                ctx.request_repaint_after(SETTLE - idle);
            }
        }
    }
}

/// Arket for en layout, ellers tegningens utstrekning.
fn sheet_or_bounds(list: &DisplayList) -> Bounds2 {
    match &list.sheet {
        Some(s) => s.bounds().union(&list.bounds),
        None => list.bounds,
    }
}

fn raster_worker(rx: Receiver<RasterRequest>, tx: Sender<RasterResult>, ctx: egui::Context) {
    while let Ok(mut req) = rx.recv() {
        // Bare den nyeste forespørselen er interessant.
        while let Ok(newer) = rx.try_recv() {
            req = newer;
        }
        let o = RasterOptions { background: req.background, ..RasterOptions::default() };
        let Some(pm) = cadcraft_render::raster::render(&req.list, &req.view, &o) else { continue };
        let size = [pm.width() as usize, pm.height() as usize];
        let image = egui::ColorImage::from_rgba_premultiplied(size, pm.data());
        if tx.send(RasterResult { id: req.id, image, world: req.view.world_bounds() }).is_err() {
            return;
        }
        ctx.request_repaint();
    }
}
