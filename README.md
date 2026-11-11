# CAD-viser for Android

En enkel, gratis og reklamefri **viser for DWG- og DXF-tegninger** på Android (og desktop, for
testing). Den bygger på de åpne bibliotekene i [CADCraft](https://github.com/storytold/cadcraft).
Appen bare viser tegninger og kan eksportere til PDF. Den endrer aldri filene, ber ikke om noen
tillatelser og bruker ikke nett.

## Funksjoner

- Åpne `.dwg` og `.dxf` fra Androids filvelger, eller fra filbehandler, e-post og Google Disk med
  «Åpne med» → **CAD-viser** (også «Del» til appen).
- Knip for å zoome, dra med én finger for å panorere, dobbelttrykk for å vise hele tegningen.
- Lagliste med av/på-bryter, «Alle på» / «Alle av» og søk når det er mange lag.
- Bytte mellom modellrom og layouter (paper space) når tegningen har layouter med innhold.
- Mørk eller lys bakgrunn (huskes).
- Eksport til PDF: papir (A4–A0 eller layoutens eget oppsett), retning, hele tegningen eller det
  som vises nå, og om linjetykkelser skal brukes. Lag som er slått av, kommer ikke med.
- «Om»-skjerm med opphavsrett og lisenser.
- Norsk grensesnitt med store trykkflater og ingen kommandolinje.

## Slik er appen bygget

```
src/
  lib.rs        oppstart for desktop og Android (android_main)
  app.rs        grensesnittet (egui): verktøylinje, gester, lag, layouter, PDF, Om
  view.rs       visningsmotor: CADCraft-visningsliste + CPU-rasterisering i bakgrunnen
  document.rs   lesing av DWG/DXF og PDF-eksport via cadcraft-io
  platform.rs   filvelger, lagring, systemlinjer: rfd på desktop, JNI på Android
  about.rs      «Om»-skjermen
android/        Gradle-prosjekt: MainActivity.java (NativeActivity) og manifest
licenses/       lisenstekster som vises i appen
testdata/       eksempeltegninger (fra CADCraft) for tester
```

- **Én Rust-crate** med [eframe/egui](https://github.com/emilk/egui) 0.37 (glow/OpenGL ES) kjører
  både på desktop og på Android (via `android-activity`/NativeActivity). Fra CADCraft brukes bare
  `cadcraft-io` (lesing og PDF), `cadcraft-render` (visningsliste og rasterisering), `cadcraft-doc`,
  `cadcraft-geom` og `cadcraft-color`. CADCraft sitt desktop-UI (`ui-egui`), `engine` og `mcp`
  brukes ikke.
- **Rendering:** visningslisten fra `cadcraft_render::build` rasteriseres med tiny-skia
  (`cadcraft_render::raster::render`, det samme som CADCraft bruker til PNG-eksport) i en
  bakgrunnstråd. Mens du knipser og drar, flyttes og skaleres det siste ferdige bildet som en
  tekstur på GPU-en. Når visningen har stått i ro i 120 ms, tegnes et skarpt bilde.
  - Fordel: hver bildeoppdatering under en geste koster det samme uansett hvor stor tegningen
    er, og tegningen ser lik ut som i CADCraft.
  - Ulempe: å tegne visningslisten som egui-figurer ville kreve ny tessellering av hele
    tegningen for hver bildeoppdatering, og det hakker på store tegninger.
  - Når du zoomer langt inn, bygges visningslisten på nytt med finere buer.
- **Lesing** skjer i en egen tråd med spinner. Feil vises som en tydelig melding på norsk, og
  panikk i leseren fanges.
- **Android-delen** (`MainActivity.java`) tar imot filer fra intents og filvelgeren. Den kopierer
  filen til appens cache og gir stien til Rust. Den lagrer også PDF med «Lagre som», og forteller
  Rust hvor store statuslinjen og navigasjonslinjen er.

### Avhengigheter og patcher

CADCraft-cratene hentes fra git og er låst til commit
`e5021f01753c5cf1d91985435d4391f40e309ece`. `Cargo.lock` tar utgangspunkt i CADCraft sin egen
låsefil, så vi bruker de samme gjennomprøvde versjonene. For eksempel er acadrust låst til
`=0.6.6`, slik CADCraft gjør.

CADCraft sin rot-`Cargo.toml` har en `[patch.crates-io]`-seksjon. Patcher arves ikke av
avhengige prosjekter, så det som trengs, er kopiert inn i vår `Cargo.toml`:

- **krilla** (PDF-skriveren `craft-pdf`, som `cadcraft-io` bruker) må patches til samme
  craft-libs-revisjon som CADCraft.
- **hayro-*** trengs ikke, fordi CADCraft bare bruker dem i tester.
- **egui/winit-forkene** trengs ikke, fordi `io`, `render` og `doc` ikke avhenger av egui. Vi
  bruker egui/eframe 0.37 og winit 0.30 fra crates.io.

## Bygge og kjøre på desktop (rask testing)

Krever Rust **1.95 eller nyere** (edition 2024).

```sh
cargo run --release -- testdata/apartment-layout.dwg
cargo test --lib            # lesing av DXF/DWG, skjulte lag, PDF, feilmeldinger
```

På Linux trengs `libxkbcommon-x11-0` og OpenGL (Mesa) for vinduet. Filer kan også dras inn i
vinduet. Musehjulet zoomer.

## Bygge APK lokalt

Du trenger:

| Verktøy | Versjon |
|---|---|
| Rust | 1.95 eller nyere, med målene `aarch64-linux-android`, `armv7-linux-androideabi`, `x86_64-linux-android` |
| cargo-ndk | 4.x (`cargo install cargo-ndk --locked`) |
| JDK | 17 |
| Android SDK | Platform `android-35`, Build-Tools `35.0.0` (compileSdk/targetSdk 35, minSdk 26 = Android 8.0) |
| Android NDK | **r27c** (`27.2.12479018`) |
| Gradle | følger med (`android/gradlew`, Gradle 8.14.3, Android Gradle Plugin 8.7.3) |

```sh
rustup target add aarch64-linux-android armv7-linux-androideabi x86_64-linux-android
cargo install cargo-ndk --locked

# SDK/NDK med sdkmanager (fra Android Studio eller «command line tools»):
sdkmanager "platforms;android-35" "build-tools;35.0.0" "ndk;27.2.12479018"

export ANDROID_HOME=$HOME/Android/Sdk
export ANDROID_NDK_HOME=$ANDROID_HOME/ndk/27.2.12479018

scripts/build-android.sh                    # alle tre ABI-er
ABIS="arm64-v8a" scripts/build-android.sh   # bare moderne telefoner (raskere)
```

Debug-APK-en havner i `android/app/build/outputs/apk/debug/app-debug.apk`. Installer med
`adb install -r …` eller ved å åpne filen på telefonen. Debug-versjonen heter «CAD-viser» med
pakkenavn `io.github.oysteineliassen.cadviewer.debug`, og kan ligge ved siden av en
release-versjon.

Debug-APK-er signeres med en fast, offentlig debug-nøkkel (`android/debug.keystore`, passord
`android`). Da kan nye debug-bygg installeres over gamle.

### Release-APK med egen nøkkel

```sh
keytool -genkeypair -v -keystore release.jks -alias cadviewer -keyalg RSA -keysize 4096 -validity 10000
export RELEASE_KEYSTORE=$PWD/release.jks RELEASE_KEYSTORE_PASSWORD=… RELEASE_KEY_ALIAS=cadviewer
scripts/build-android.sh    # bygger også app-release.apk når RELEASE_KEYSTORE er satt
```

Ikke legg `release.jks` i repoet (`.gitignore` hindrer det). Ta godt vare på nøkkelen: oppdateringer
må signeres med samme nøkkel.

## GitHub Actions

`.github/workflows/android.yml` kjører ved hver push (også `main`), ved pull requests og ved
manuell start. Den kjører testene, bygger APK for arm64-v8a, armeabi-v7a og x86_64 og laster
opp **CAD-viser-debug-apk** som artefakt. Artefakter ligger under Actions → kjøringen →
Artifacts.

For en signert **release-APK** legger du inn disse under *Settings → Secrets and variables →
Actions*:

| Secret | Innhold |
|---|---|
| `RELEASE_KEYSTORE_BASE64` | `base64 -w0 release.jks` |
| `RELEASE_KEYSTORE_PASSWORD` | passordet til nøkkellageret |
| `RELEASE_KEY_ALIAS` | aliaset, f.eks. `cadviewer` |
| `RELEASE_KEY_PASSWORD` | nøkkelpassordet (kan utelates hvis det er det samme) |

Med disse på plass lastes også **CAD-viser-release-apk** opp.

## Lisens

Koden i dette repoet er lisensiert under MIT (se `LICENSE`).

Appen bygger på:

- **CADCraft**: © 2026 ArtCraft Team og CADCraft-bidragsyterne, MIT OR Apache-2.0.
  Se `licenses/CADCRAFT-NOTICE`, `licenses/CADCRAFT-LICENSE-MIT` og
  `licenses/CADCRAFT-LICENSE-APACHE`.
- **acadrust** (DWG-lesing): Mozilla Public License 2.0. Brukes uendret som eget bibliotek.
  Kildekoden finnes på [crates.io](https://crates.io/crates/acadrust) og
  [GitHub](https://github.com/hakanaktt/acadrust). Se `licenses/ACADRUST-LICENSE-MPL-2.0` og
  `licenses/ACADRUST-THIRD_PARTY_NOTICES.md`.
- **egui**-fontene (Ubuntu, Noto Emoji, Hack, emoji-icon-font): se `licenses/EGUI-FONTS.txt`.
- Alle Rust-biblioteker i appen med lisens: `licenses/THIRD-PARTY.md`. Lag den på nytt med
  `python3 scripts/third_party.py` når `Cargo.lock` endres.

Alt dette vises også på «Om»-skjermen i appen. Eksempeltegningene i `testdata/` kommer fra
CADCraft (se `testdata/README.md`).

CAD-viser er ikke tilknyttet, sponset eller godkjent av Autodesk, Inc. AutoCAD og DWG er
varemerker for Autodesk, Inc.
