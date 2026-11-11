//! «Om»-skjermen: hva appen er, og opphavsrett og lisenser for det den bygger på.

use eframe::egui::{self, RichText};

const CADCRAFT_NOTICE: &str = include_str!("../licenses/CADCRAFT-NOTICE");
const CADCRAFT_MIT: &str = include_str!("../licenses/CADCRAFT-LICENSE-MIT");
const CADCRAFT_APACHE: &str = include_str!("../licenses/CADCRAFT-LICENSE-APACHE");
const ACADRUST_MPL: &str = include_str!("../licenses/ACADRUST-LICENSE-MPL-2.0");
const ACADRUST_THIRD_PARTY: &str = include_str!("../licenses/ACADRUST-THIRD_PARTY_NOTICES.md");
const EGUI_FONTS: &str = include_str!("../licenses/EGUI-FONTS.txt");
const THIRD_PARTY: &str = include_str!("../licenses/THIRD-PARTY.md");
const APP_LICENSE: &str = include_str!("../LICENSE");

pub const CADCRAFT_REV: &str = "e5021f01753c5cf1d91985435d4391f40e309ece";

pub fn show(ui: &mut egui::Ui) {
    ui.label(RichText::new(format!("CAD-viser {}", env!("CARGO_PKG_VERSION"))).heading());
    ui.label("En enkel, gratis og reklamefri viser for DWG- og DXF-tegninger. Appen bare viser tegninger og kan eksportere dem til PDF; den endrer aldri filene dine, og ingenting sendes over nett.");
    ui.add_space(8.0);
    ui.label(RichText::new("Bygget på CADCraft").strong());
    ui.label(format!(
        "Lesing, tegning og PDF-eksport gjøres av de åpne bibliotekene i CADCraft (https://github.com/storytold/cadcraft, commit {}), lisensiert under MIT eller Apache-2.0.",
        &CADCRAFT_REV[..10]
    ));
    ui.label("DWG-filer leses med acadrust (https://github.com/hakanaktt/acadrust), lisensiert under Mozilla Public License 2.0 og brukt uendret som eget bibliotek. Kildekoden er tilgjengelig fra crates.io og lenken over.");
    ui.label("CAD-viser er ikke tilknyttet, sponset eller godkjent av Autodesk, Inc. AutoCAD og DWG er varemerker for Autodesk, Inc.");
    ui.add_space(8.0);
    for (title, text) in [
        ("Lisens for CAD-viser (MIT)", APP_LICENSE),
        ("CADCraft NOTICE", CADCRAFT_NOTICE),
        ("CADCraft: MIT-lisens", CADCRAFT_MIT),
        ("CADCraft: Apache-lisens 2.0", CADCRAFT_APACHE),
        ("acadrust: Mozilla Public License 2.0", ACADRUST_MPL),
        ("acadrust: tredjepartsmerknader", ACADRUST_THIRD_PARTY),
        ("Skrifttyper i grensesnittet (egui)", EGUI_FONTS),
        ("Alle Rust-biblioteker i appen", THIRD_PARTY),
    ] {
        egui::CollapsingHeader::new(RichText::new(title).size(19.0)).show(ui, |ui| {
            ui.add(egui::Label::new(RichText::new(text).monospace().size(12.0)).wrap());
        });
    }
}
