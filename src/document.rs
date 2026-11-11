//! Lese en DWG/DXF-fil i bakgrunnen, og skrive PDF.

use std::path::Path;
use std::sync::Arc;

use cadcraft_doc::{Drawing, Space};
use cadcraft_geom::Bounds2;
use cadcraft_io::{IoError, PdfOptions};

/// En åpnet tegning.
pub struct Document {
    pub name: String,
    pub drawing: Arc<Drawing>,
    /// Modellrom og layoutene som har innhold, i fanerekkefølge.
    pub spaces: Vec<Space>,
}

impl Document {
    pub fn space_label(space: &Space) -> String {
        match space {
            Space::Model => "Modell".into(),
            Space::Paper(n) => n.clone(),
        }
    }
}

/// Les filen. Kjøres i en bakgrunnstråd; panikk i en leser blir til en feilmelding.
pub fn load(path: &Path, name: &str) -> Result<Document, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("Kunne ikke lese filen: {e}"))?;
    if bytes.is_empty() {
        return Err("Filen er tom.".into());
    }
    let parse_name = if name.is_empty() { path.to_string_lossy().into_owned() } else { name.to_owned() };
    let result = std::panic::catch_unwind(|| cadcraft_io::read(&bytes, &parse_name));
    let drawing = match result {
        Ok(Ok(d)) => d,
        Ok(Err(IoError::Unsupported(ext))) => {
            let what = if ext.is_empty() { "denne filtypen".to_owned() } else { format!("«.{ext}»-filer") };
            return Err(format!("Appen kan ikke åpne {what}. Bare DWG- og DXF-tegninger støttes."));
        }
        Ok(Err(e)) => return Err(format!("Filen ser ikke ut til å være en gyldig DWG- eller DXF-tegning, eller den er skadet.\n\nDetaljer: {e}")),
        Err(_) => return Err("Det oppstod en intern feil under lesing av filen. Den kan være skadet eller i et format som ikke støttes.".into()),
    };
    let mut spaces = vec![Space::Model];
    for l in drawing.paper_space_layouts() {
        if !l.name.eq_ignore_ascii_case("Model") && !l.entities.is_empty() {
            spaces.push(Space::Paper(l.name.clone()));
        }
    }
    // Modellrommet kan være tomt (alt ligger i layouter): start da i første layout.
    if drawing.model.is_empty() && spaces.len() > 1 {
        spaces.remove(0);
        spaces.push(Space::Model);
    }
    let display = if name.is_empty() { path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default() } else { name.to_owned() };
    Ok(Document { name: display, drawing: Arc::new(drawing), spaces })
}

/// Papirformat for PDF-eksport.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Paper {
    /// Layoutens eget sideoppsett (bare for layouter).
    Layout,
    A4,
    A3,
    A2,
    A1,
    A0,
}

impl Paper {
    pub const ALL: [Paper; 6] = [Paper::Layout, Paper::A4, Paper::A3, Paper::A2, Paper::A1, Paper::A0];
    pub fn label(self) -> &'static str {
        match self {
            Paper::Layout => "Som i layouten",
            Paper::A4 => "A4",
            Paper::A3 => "A3",
            Paper::A2 => "A2",
            Paper::A1 => "A1",
            Paper::A0 => "A0",
        }
    }
    fn name(self) -> Option<&'static str> {
        match self {
            Paper::Layout => None,
            Paper::A4 => Some("A4"),
            Paper::A3 => Some("A3"),
            Paper::A2 => Some("A2"),
            Paper::A1 => Some("A1"),
            Paper::A0 => Some("A0"),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Orientation {
    Auto,
    Landscape,
    Portrait,
}

#[derive(Clone, Debug)]
pub struct PdfChoice {
    pub paper: Paper,
    pub orientation: Orientation,
    /// Bare det som vises på skjermen (modellrom), ellers hele tegningen.
    pub only_visible: bool,
    pub lineweights: bool,
}

impl Default for PdfChoice {
    fn default() -> Self {
        PdfChoice { paper: Paper::A4, orientation: Orientation::Auto, only_visible: false, lineweights: true }
    }
}

/// Lag PDF av et rom. Lag som er slått av i appen tas ikke med. `extents` er det som skal med
/// når hele tegningen eksporteres, `window` det som vises på skjermen.
pub fn export_pdf(d: &Drawing, space: &Space, c: &PdfChoice, title: &str, extents: Bounds2, window: Bounds2) -> Result<Vec<u8>, String> {
    let model = matches!(space, Space::Model);
    let area = if model && c.only_visible { window } else { extents };
    let landscape = match c.orientation {
        Orientation::Landscape => Some(true),
        Orientation::Portrait => Some(false),
        Orientation::Auto if model || c.paper != Paper::Layout => Some(area.width() >= area.height()),
        Orientation::Auto => None,
    };
    let use_layout_page = !model && c.paper == Paper::Layout;
    let o = PdfOptions {
        paper: c.paper.name().map(str::to_owned).or_else(|| (!use_layout_page).then(|| "A4".to_owned())),
        landscape: if use_layout_page { None } else { landscape },
        // Modellrom tilpasses arket. En layout skrives 1:1 på sitt eget ark, ellers tilpasset.
        fit: Some(model || !use_layout_page),
        window: (model && c.only_visible).then_some(window),
        lineweights: Some(c.lineweights),
        compress: true,
        title: title.to_owned(),
        // Små marger så tegningen ikke går helt ut i kanten (modellrom).
        margins: model.then_some([10.0, 10.0, 10.0, 10.0]),
        ..PdfOptions::default()
    };
    if let Some(w) = &o.window {
        cadcraft_io::check_window(w).map_err(|e| format!("Ugyldig område: {e}"))?;
    }
    match std::panic::catch_unwind(|| cadcraft_io::pdf(d, space, &o)) {
        Ok(Ok(bytes)) => Ok(bytes),
        Ok(Err(e)) => Err(format!("Kunne ikke lage PDF: {e}")),
        Err(_) => Err("Det oppstod en intern feil under PDF-eksporten.".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const APARTMENT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/testdata/apartment.dxf");

    fn tmp(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("cadviewer-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join(name)
    }

    fn extents(d: &Drawing, s: &Space) -> Bounds2 {
        let list = cadcraft_render::build(d, s, &cadcraft_render::Options::default());
        assert!(!list.prims.is_empty(), "tomt rom {s:?}");
        list.sheet.map_or(list.bounds, |sh| sh.bounds())
    }

    #[test]
    fn opens_dxf() {
        let doc = load(Path::new(APARTMENT), "apartment.dxf").unwrap();
        assert_eq!(doc.name, "apartment.dxf");
        assert_eq!(doc.spaces[0], Space::Model);
        assert!(doc.drawing.layers.len() > 3);
        eprintln!("rom: {:?}", doc.spaces);
        eprintln!("lag: {:?}", doc.drawing.layers.iter().map(|l| &l.name).collect::<Vec<_>>());
    }

    #[test]
    fn opens_dwg_written_from_the_example() {
        let doc = load(Path::new(APARTMENT), "apartment.dxf").unwrap();
        let dwg = cadcraft_io::write(&doc.drawing, "apartment.dwg").unwrap();
        assert!(dwg.starts_with(b"AC10"));
        let path = tmp("apartment.dwg");
        std::fs::write(&path, &dwg).unwrap();
        let back = load(&path, "apartment.dwg").unwrap();
        assert_eq!(back.drawing.model.len(), doc.drawing.model.len());
        assert_eq!(back.spaces.len(), doc.spaces.len());
        let b = extents(&back.drawing, &Space::Model);
        assert!(b.width() > 1000.0);
    }

    #[test]
    fn hidden_layers_are_not_drawn() {
        let doc = load(Path::new(APARTMENT), "apartment.dxf").unwrap();
        let all = cadcraft_render::build(&doc.drawing, &Space::Model, &cadcraft_render::Options::default()).prims.len();
        let mut d = (*doc.drawing).clone();
        for l in &mut d.layers {
            l.on = false;
        }
        let none = cadcraft_render::build(&d, &Space::Model, &cadcraft_render::Options::default()).prims.len();
        assert!(all > 0);
        assert_eq!(none, 0);
    }

    #[test]
    fn exports_pdf_for_every_space() {
        let doc = load(Path::new(APARTMENT), "apartment.dxf").unwrap();
        for s in &doc.spaces {
            let ext = extents(&doc.drawing, s);
            for c in [
                PdfChoice::default(),
                PdfChoice { paper: Paper::Layout, ..PdfChoice::default() },
                PdfChoice { paper: Paper::A3, orientation: Orientation::Portrait, only_visible: true, lineweights: false },
            ] {
                let window = Bounds2::new(ext.min, ext.center());
                let pdf = export_pdf(&doc.drawing, s, &c, "apartment", ext, window).unwrap();
                assert!(pdf.starts_with(b"%PDF"), "{s:?} {c:?}");
                assert!(pdf.len() > 1000);
            }
        }
        std::fs::write(tmp("apartment.pdf"), export_pdf(&doc.drawing, &Space::Model, &PdfChoice::default(), "apartment", extents(&doc.drawing, &Space::Model), Bounds2::default()).unwrap()).unwrap();
    }

    #[test]
    fn garbage_gives_a_clear_error() {
        let path = tmp("tull.dwg");
        std::fs::write(&path, b"AC1032 dette er ikke en ekte tegning").unwrap();
        let e = load(&path, "tull.dwg").err().unwrap();
        assert!(e.contains("DWG") || e.contains("feil"), "{e}");
        let path = tmp("bilde.jpg");
        std::fs::write(&path, [0xff, 0xd8, 0xff, 0xe0, 1, 2, 3, 4, 5]).unwrap();
        let e = load(&path, "bilde.jpg").err().unwrap();
        assert!(e.contains("støttes"), "{e}");
        let path = tmp("tom.dxf");
        std::fs::write(&path, b"").unwrap();
        assert_eq!(load(&path, "tom.dxf").err().unwrap(), "Filen er tom.");
    }
}
