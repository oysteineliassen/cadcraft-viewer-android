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

/// Ledig og totalt minne i byte (Linux og Android: /proc/meminfo). `None` der det ikke finnes.
pub fn memory() -> Option<(u64, u64)> {
    let text = std::fs::read_to_string("/proc/meminfo").ok()?;
    let field = |name: &str| {
        text.lines()
            .find(|l| l.starts_with(name))
            .and_then(|l| l.split_whitespace().nth(1))
            .and_then(|v| v.parse::<u64>().ok())
            .map(|kb| kb * 1024)
    };
    Some((field("MemAvailable:")?, field("MemTotal:")?))
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

    pub fn set_light_bars(_light: bool) {}
}

#[cfg(target_os = "android")]
mod imp {
    use super::{Event, push};
    use jni::JNIEnv;
    use jni::objects::{JObject, JString, JValue};
    use std::path::PathBuf;
    use std::sync::Mutex;

    pub static DATA_DIR: Mutex<Option<PathBuf>> = Mutex::new(None);
    /// JavaVM og aktiviteten (globale JNI-referanser fra android-activity), satt i android_main.
    /// ndk_context peker på Application, ikke aktiviteten, så den kan ikke brukes her.
    static ACTIVITY: Mutex<Option<(usize, usize)>> = Mutex::new(None);

    pub fn set_activity(vm: *mut std::ffi::c_void, activity: *mut std::ffi::c_void) {
        if let Ok(mut a) = ACTIVITY.lock() {
            *a = Some((vm as usize, activity as usize));
        }
    }

    type JniCall = Box<dyn FnOnce(&mut JNIEnv, &JObject) -> jni::errors::Result<()> + Send>;

    /// Kø til JNI-tråden. Kall til Java gjøres aldri fra egui-tråden: den behandler
    /// livssyklus-hendelser som Androids UI-tråd venter på, så et JNI-kall som venter på
    /// UI-tråden der, gir vranglås (ANR). JNI-tråden kan vente uten å stoppe appen.
    static JNI_QUEUE: Mutex<Option<std::sync::mpsc::Sender<JniCall>>> = Mutex::new(None);

    fn jni_worker(rx: std::sync::mpsc::Receiver<JniCall>) {
        let Some((vm, activity)) = ACTIVITY.lock().ok().and_then(|a| *a) else { return };
        // SAFETY: pekeren er prosessens JavaVM fra AndroidApp::vm_as_ptr.
        let Ok(vm) = (unsafe { jni::JavaVM::from_raw(vm as *mut jni::sys::JavaVM) }) else { return };
        // Tråden er festet til JVM-en så lenge den lever.
        let Ok(mut env) = vm.attach_current_thread_permanently() else { return };
        // SAFETY: global referanse til aktiviteten (AndroidApp::activity_as_ptr), gyldig så lenge
        // appen kjører. JObject har ingen Drop, så referansen frigis ikke her.
        let activity = unsafe { JObject::from_raw(activity as jni::sys::jobject) };
        while let Ok(call) = rx.recv() {
            if let Err(e) = env.with_local_frame(16, |env| call(env, &activity)) {
                let detail = describe_exception(&mut env);
                log::error!("JNI-kall feilet: {e}: {detail}");
            }
        }
    }

    /// Kall en metode på aktiviteten (MainActivity.java) fra JNI-tråden.
    fn call_activity(f: impl FnOnce(&mut JNIEnv, &JObject) -> jni::errors::Result<()> + Send + 'static) {
        let Ok(mut q) = JNI_QUEUE.lock() else { return };
        if q.is_none() {
            let (tx, rx) = std::sync::mpsc::channel::<JniCall>();
            if std::thread::Builder::new().name("jni".into()).spawn(move || jni_worker(rx)).is_err() {
                log::error!("JNI: kunne ikke starte tråd");
                return;
            }
            *q = Some(tx);
        }
        if let Some(tx) = q.as_ref() {
            let _ = tx.send(Box::new(f));
        }
    }

    /// Teksten til en ventende Java-exception (og fjern den).
    fn describe_exception(env: &mut JNIEnv) -> String {
        let Ok(ex) = env.exception_occurred() else { return String::new() };
        if ex.is_null() {
            return String::new();
        }
        let _ = env.exception_clear();
        let text = env
            .call_method(&ex, "toString", "()Ljava/lang/String;", &[])
            .and_then(|v| v.l())
            .map(JString::from)
            .ok()
            .and_then(|s| env.get_string(&s).ok().map(String::from))
            .unwrap_or_default();
        let _ = env.exception_clear();
        text
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
        call_activity(move |env, a| {
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

    /// Mørke ikoner i status-/navigasjonslinjen på lys bakgrunn, lyse på mørk.
    pub fn set_light_bars(light: bool) {
        call_activity(move |env, a| env.call_method(a, "setLightBars", "(Z)V", &[JValue::Bool(u8::from(light))]).map(|_| ()));
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

pub use imp::{data_dir, leave, pick_file, save_pdf, set_light_bars};

#[cfg(target_os = "android")]
pub use imp::{DATA_DIR, set_activity};
