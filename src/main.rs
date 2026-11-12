//! Desktop-versjonen (for rask testing): `cadcraft-viewer [fil.dwg|fil.dxf]`.

fn main() -> eframe::Result {
    // CADCraft sin DWG-leser i en egen prosess (en kopi av programmet startet med --dwg-worker):
    // minnet den bruker frigis når den er ferdig, og et krasj i den tar ikke med seg appen.
    if let Some(code) = cadcraft_io::dwg_worker::serve_if_asked() {
        std::process::exit(code);
    }
    cadcraft_io::dwg_worker::use_own_worker();
    env_logger::init();
    let open = std::env::args_os().nth(1).filter(|a| a != cadcraft_io::dwg_worker::WORKER_ARG).map(std::path::PathBuf::from);
    cadcraft_viewer::run_desktop(open)
}
