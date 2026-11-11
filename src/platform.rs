//! Det som er forskjellig på desktop og Android: filvelger, lagring av PDF, systemmeldinger,
//! skjermkanter (statuslinje/navigasjonslinje) og hvor innstillinger lagres.
//!
//! Plattformen sender hendelser til appen gjennom en felles kø ([`push`] / [`take`]), slik at
//! Java-koden på Android kan levere filer før appen har startet ferdig.

use std::path::PathBuf;
use std::sync::Mutex;

use eframe::egui;

pub enum Event {
    /// En fil skal åpnes. `path` er en lokal kopi, `name` er visningsnavnet (med filendelse).
    Open { path: PathBuf, name: String },
    /// En melding som skal vises kort (f.eks. «PDF lagret»).
    Message(String),
}

static EVENTS: Mutex<Vec<Event>> = Mutex::new(Vec::new());
static CTX: Mutex<Option<egui::Context>> = Mutex::new(None);
/// Avstand fra skjermkantene som dekkes av systemlinjer, i fysiske piksler: venstre, topp,
/// høyre, bunn.
static INSETS: Mutex<[f32; 4]> = Mutex::new([0.0; 4]);

pub fn push(e: Event) {
    if let Ok(mut q) = EVENTS.lock() {
        q.push(e);
    }
    repaint();
}

pub fn take() -> Vec<Event> {
    EVENTS.lock().map(|mut q| std::mem::take(&mut *q)).unwrap_or_default()
}

pub fn set_context(ctx: &egui::Context) {
    if let Ok(mut c) = CTX.lock() {
        *c = Some(ctx.clone());
    }
}

pub fn repaint() {
    if let Ok(c) = CTX.lock()
        && let Some(ctx) = c.as_ref()
    {
        ctx.request_repaint();
    }
}

#[allow(dead_code)]
pub fn set_insets(insets: [f32; 4]) {
    if let Ok(mut i) = INSETS.lock() {
        *i = insets;
    }
    repaint();
}

/// Systemlinjenes størrelse i fysiske piksler (venstre, topp, høyre, bunn).
pub fn insets() -> [f32; 4] {
    INSETS.lock().map(|i| *i).unwrap_or_default()
}

#[cfg(not(target_os = "android"))]
mod imp {
    use super::{Event, push};
    use std::path::PathBuf;

    pub fn pick_file() {
        std::thread::spawn(|| {
            let file = rfd::FileDialog::new()
                .set_title("Åpne tegning")
                .add_filter("Tegninger (DWG, DXF)", &["dwg", "dxf", "DWG", "DXF"])
                .add_filter("Alle filer", &["*"])
                .pick_file();
            if let Some(path) = file {
                let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                push(Event::Open { path, name });
            }
        });
    }

    pub fn save_pdf(name: String, bytes: Vec<u8>) {
        std::thread::spawn(move || {
            let file = rfd::FileDialog::new().set_title("Lagre PDF").add_filter("PDF", &["pdf"]).set_file_name(&name).save_file();
            if let Some(path) = file {
                match std::fs::write(&path, &bytes) {
                    Ok(()) => push(Event::Message(format!("PDF lagret: {}", path.display()))),
                    Err(e) => push(Event::Message(format!("Kunne ikke lagre PDF: {e}"))),
                }
            }
        });
    }

    pub fn data_dir() -> Option<PathBuf> {
        let base = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("APPDATA").map(PathBuf::from))
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
        Some(base.join("cadcraft-viewer"))
    }

    /// Tilbake-knappen når ingenting annet er åpent: ingenting å gjøre på desktop.
    pub fn leave() {}
}

#[cfg(target_os = "android")]
mod imp {
    use super::{Event, push};
    use jni::JNIEnv;
    use jni::objects::{JObject, JString, JValue};
    use std::path::PathBuf;
    use std::sync::Mutex;

    pub static DATA_DIR: Mutex<Option<PathBuf>> = Mutex::new(None);

    /// Kall en metode på aktiviteten (MainActivity.java).
    fn call_activity(f: impl FnOnce(&mut JNIEnv, &JObject) -> jni::errors::Result<()>) {
        let ctx = ndk_context::android_context();
        // SAFETY: android-activity setter ndk_context til prosessens JavaVM og aktiviteten.
        let Ok(vm) = (unsafe { jni::JavaVM::from_raw(ctx.vm().cast()) }) else { return };
        let Ok(mut env) = vm.attach_current_thread() else { return };
        // SAFETY: et globalt referanse-objekt til aktiviteten som lever like lenge som appen.
        let activity = unsafe { JObject::from_raw(ctx.context().cast()) };
        if let Err(e) = f(&mut env, &activity) {
            log::error!("JNI-kall feilet: {e}");
            let _ = env.exception_describe();
            let _ = env.exception_clear();
        }
    }

    pub fn pick_file() {
        call_activity(|env, a| env.call_method(a, "openPicker", "()V", &[]).map(|_| ()));
    }

    pub fn save_pdf(name: String, bytes: Vec<u8>) {
        let Some(dir) = data_dir() else {
            push(Event::Message("Fant ingen mappe å skrive PDF-en til.".into()));
            return;
        };
        let path = dir.join("eksport.pdf");
        if let Err(e) = std::fs::create_dir_all(&dir).and_then(|()| std::fs::write(&path, &bytes)) {
            push(Event::Message(format!("Kunne ikke skrive PDF: {e}")));
            return;
        }
        call_activity(|env, a| {
            let p = env.new_string(path.to_string_lossy())?;
            let n = env.new_string(&name)?;
            env.call_method(a, "savePdf", "(Ljava/lang/String;Ljava/lang/String;)V", &[JValue::Object(&p), JValue::Object(&n)])
                .map(|_| ())
        });
    }

    pub fn data_dir() -> Option<PathBuf> {
        DATA_DIR.lock().ok().and_then(|d| d.clone())
    }

    /// Tilbake-knappen når ingenting annet er åpent: legg appen i bakgrunnen.
    pub fn leave() {
        call_activity(|env, a| env.call_method(a, "moveTaskToBack", "(Z)Z", &[JValue::Bool(1)]).map(|_| ()));
    }

    fn string(env: &mut JNIEnv, s: &JString) -> String {
        env.get_string(s).map(String::from).unwrap_or_default()
    }

    /// Fra MainActivity.java: en fil er kopiert til appens cache og skal åpnes.
    #[unsafe(no_mangle)]
    pub extern "system" fn Java_io_github_oysteineliassen_cadviewer_MainActivity_nativeOpenFile<'l>(
        mut env: JNIEnv<'l>,
        _this: JObject<'l>,
        path: JString<'l>,
        name: JString<'l>,
    ) {
        let path = string(&mut env, &path);
        let name = string(&mut env, &name);
        push(Event::Open { path: PathBuf::from(path), name });
    }

    /// Fra MainActivity.java: en melding til brukeren (f.eks. at en fil ikke kunne kopieres).
    #[unsafe(no_mangle)]
    pub extern "system" fn Java_io_github_oysteineliassen_cadviewer_MainActivity_nativeMessage<'l>(
        mut env: JNIEnv<'l>,
        _this: JObject<'l>,
        text: JString<'l>,
    ) {
        let text = string(&mut env, &text);
        push(Event::Message(text));
    }

    /// Fra MainActivity.java: systemlinjenes størrelse i piksler.
    #[unsafe(no_mangle)]
    pub extern "system" fn Java_io_github_oysteineliassen_cadviewer_MainActivity_nativeInsets<'l>(
        _env: JNIEnv<'l>,
        _this: JObject<'l>,
        left: i32,
        top: i32,
        right: i32,
        bottom: i32,
    ) {
        super::set_insets([left as f32, top as f32, right as f32, bottom as f32]);
    }
}

pub use imp::{data_dir, leave, pick_file, save_pdf};

#[cfg(target_os = "android")]
pub use imp::DATA_DIR;
