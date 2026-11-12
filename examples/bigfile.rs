//! Lager en stor DWG (mange kopier av apartment-planen) og måler lesing og tegning.
//! `cargo run --release --example bigfile -- lag N ut.dwg` / `cargo run --release --example bigfile -- les fil.dwg`
use cadcraft_doc::Space;
use std::time::Instant;

fn rss_mb() -> u64 {
    std::fs::read_to_string("/proc/self/status").ok().and_then(|s| s.lines().find(|l| l.starts_with("VmHWM")).and_then(|l| l.split_whitespace().nth(1)?.parse::<u64>().ok())).unwrap_or(0) / 1024
}

fn cadcraft_dwg_is(b: &[u8]) -> bool { b.starts_with(b"AC10") }

fn main() {
    let a: Vec<String> = std::env::args().collect();
    match a[1].as_str() {
        "lag" => {
            let n: usize = a[2].parse().unwrap();
            let src = std::fs::read("testdata/apartment.dxf").unwrap();
            let mut d = cadcraft_io::read(&src, "a.dxf").unwrap();
            let ents: Vec<_> = d.model.iter().cloned().collect();
            for i in 1..n {
                let dx = (i % 40) as f64 * 16000.0;
                let dy = (i / 40) as f64 * 11000.0;
                for e in &ents {
                    let mut e = (**e).clone();
                    e.kind.transform(&cadcraft_geom::Mat3::translate(cadcraft_geom::Vec2::new(dx, dy)));
                    let _ = d.add(&Space::Model, e.common.clone(), e.kind.clone());
                }
            }
            let bytes = cadcraft_io::write(&d, &a[3]).unwrap();
            std::fs::write(&a[3], &bytes).unwrap();
            println!("{} objekter, {} MB", d.model.len(), bytes.len() / 1_000_000);
        }
        "dek" => {
            let bytes = std::fs::read(&a[2]).unwrap();
            let (dxf, _) = cadcraft_dwg::decode(&bytes, cadcraft_dwg::Limits::DEFAULT).unwrap();
            println!("bare DWG→DXF: {} MB DXF, maks minne {} MB", dxf.len() / 1_000_000, rss_mb());
            std::fs::write(&a[3], &dxf).unwrap();
        }
        _ => {
            let t = Instant::now();
            let bytes = std::fs::read(&a[2]).unwrap();
            let d = cadcraft_io::read(&bytes, &a[2]).unwrap();
            println!("lest: {} objekter på {:?}, maks minne {} MB", d.model.len(), t.elapsed(), rss_mb());
            if cadcraft_dwg_is(&bytes) {
                let (dxf, _) = cadcraft_dwg::decode(&bytes, cadcraft_dwg::Limits::DEFAULT).unwrap();
                println!("DWG→DXF: {} MB DXF", dxf.len() / 1_000_000);
            }
            let t = Instant::now();
            let coarse = cadcraft_render::build(&d, &Space::Model, &cadcraft_render::Options { tolerance: 1e3, min_dash: 1e3, text: false, ..Default::default() });
            let b = coarse.bounds;
            let px = 2000.0 / b.width().max(b.height());
            drop(coarse);
            let l = cadcraft_render::build(&d, &Space::Model, &cadcraft_render::Options { tolerance: 0.2 / px, min_dash: 2.0 / px, transparency: true, ..Default::default() });
            println!("visningsliste: {} primitiver på {:?}, maks minne {} MB", l.prims.len(), t.elapsed(), rss_mb());
        }
    }
}
