//! Desktop-versjonen (for rask testing): `cadcraft-viewer [fil.dwg|fil.dxf]`.

fn main() -> eframe::Result {
    env_logger::init();
    let open = std::env::args_os().nth(1).map(std::path::PathBuf::from);
    cadcraft_viewer::run_desktop(open)
}
