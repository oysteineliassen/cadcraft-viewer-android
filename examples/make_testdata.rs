//! Lager testfilene i `testdata/` fra CADCraft sitt eksempel `apartment.dxf`:
//! - `apartment.dwg`: samme tegning skrevet som DWG (for å teste DWG-lesing),
//! - `apartment-layout.dxf` / `.dwg`: med en layout som viser planen gjennom et visningsvindu.
//!
//! `cargo run --example make_testdata`

use cadcraft_doc::{Common, EntityKind, Space, Viewport};
use cadcraft_geom::Vec2;

fn main() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("testdata");
    let src = std::fs::read(dir.join("apartment.dxf")).expect("testdata/apartment.dxf");
    let mut d = cadcraft_io::read(&src, "apartment.dxf").expect("les apartment.dxf");
    std::fs::write(dir.join("apartment.dwg"), cadcraft_io::write(&d, "apartment.dwg").expect("skriv DWG")).unwrap();

    let model = cadcraft_render::build(&d, &Space::Model, &cadcraft_render::Options::default()).bounds;
    let name = d.paper_space_layouts().first().map(|l| l.name.clone()).expect("en layout");
    let sheet = cadcraft_render::paper::sheet(&d, &name).expect("ark");
    let area = sheet.printable;
    let (w, h) = (area.width() * 0.9, area.height() * 0.8);
    let center = Vec2::new(area.center().x, area.min.y + area.height() * 0.55);
    // Visningsvindu som viser hele planen.
    let view_height = model.height().max(model.width() * h / w) * 1.05;
    let vp = Viewport { center: center.to3(0.0), width: w, height: h, view_center: model.center(), view_height, id: 2, ..Viewport::default() };
    let paper = Space::Paper(name.clone());
    d.add(&paper, Common { layer: "0".into(), ..Common::default() }, EntityKind::Viewport(vp)).expect("viewport");
    std::fs::write(dir.join("apartment-layout.dxf"), cadcraft_io::write(&d, "apartment-layout.dxf").expect("skriv DXF")).unwrap();
    std::fs::write(dir.join("apartment-layout.dwg"), cadcraft_io::write(&d, "apartment-layout.dwg").expect("skriv DWG")).unwrap();
    println!("layout «{name}», ark {:?}", sheet.size);
}
