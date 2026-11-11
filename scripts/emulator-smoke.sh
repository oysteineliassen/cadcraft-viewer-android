#!/usr/bin/env bash
# Røyktest på en kjørende emulator/telefon (adb): installer debug-APK-en, start appen, åpne en
# DWG via «Åpne med»-intent, ta skjermbilder og sjekk at appen fortsatt kjører uten panikk.
#
#   scripts/emulator-smoke.sh path/til/app-debug.apk [utmappe]
set -euo pipefail
APK="$1"
OUT="${2:-smoke}"
PKG=io.github.oysteineliassen.cadviewer.debug
ACT=io.github.oysteineliassen.cadviewer.MainActivity
cd "$(dirname "$0")/.."
mkdir -p "$OUT"

shot() { adb exec-out screencap -p > "$OUT/$1.png"; }
alive() {
  if ! adb shell pidof "$PKG" > /dev/null; then
    echo "::error::Appen kjører ikke lenger ($1)"
    adb logcat -d > "$OUT/logcat.txt" || true
    exit 1
  fi
}

adb logcat -c || true
adb install -r "$APK"
adb shell settings put system screen_off_timeout 600000 || true

# 1. Vanlig oppstart: startskjermen.
adb shell am start -W -n "$PKG/$ACT"
sleep 8
alive "oppstart"
shot 1-start

# 2. «Åpne med»: DWG-fil fra appens egen mappe (file://-URI, som eldre filbehandlere sender).
adb push testdata/apartment-layout.dwg /data/local/tmp/apartment-layout.dwg
adb shell chmod 644 /data/local/tmp/apartment-layout.dwg
adb shell run-as "$PKG" mkdir -p files/test
adb shell run-as "$PKG" cp /data/local/tmp/apartment-layout.dwg files/test/apartment-layout.dwg
adb shell am start -W -a android.intent.action.VIEW \
  -d "file:///data/data/$PKG/files/test/apartment-layout.dwg" -t application/acad -n "$PKG/$ACT"
sleep 12
alive "åpning av DWG"
shot 2-dwg
echo "--- cache/open ---"
adb shell run-as "$PKG" ls -la cache/open || true

# 3. Dobbelttrykk midt på skjermen (vis hele tegningen) og en sveip (panorering).
read -r W H < <(adb shell wm size | sed -n 's/.*: \([0-9]*\)x\([0-9]*\).*/\1 \2/p' | tail -1)
adb shell input tap $((W / 2)) $((H / 2)); adb shell input tap $((W / 2)) $((H / 2))
sleep 2
adb shell input swipe $((W / 2)) $((H / 2)) $((W / 3)) $((H / 3)) 400
sleep 4
alive "gester"
shot 3-gester

# 4. Rotasjon til liggende.
adb shell settings put system accelerometer_rotation 0 || true
adb shell settings put system user_rotation 1 || true
sleep 6
alive "rotasjon"
shot 4-liggende
adb shell settings put system user_rotation 0 || true

adb logcat -d > "$OUT/logcat.txt" || true
PID=$(adb shell pidof "$PKG" | tr -d '\r')
echo "--- appens logg (pid $PID) ---"
adb logcat -d --pid="$PID" | grep -v -E "AppsFilter|InputDispatcher" | tail -80 || true

# Nedskalerte skjermbilder i loggen, så de kan ses uten å laste ned artefakter
# (dekod med: base64 -d > bilde.png).
for f in 1-start 2-dwg 3-gester 4-liggende; do
  if command -v convert > /dev/null; then
    convert "$OUT/$f.png" -resize 270x "$OUT/$f-liten.png"
    echo "SKJERMBILDE $f: $(base64 -w0 "$OUT/$f-liten.png")"
  fi
done

fail=0
if grep -A3 "FATAL EXCEPTION" "$OUT/logcat.txt" | grep -q "Process: $PKG" \
   || grep -E "panicked|SIGSEGV|SIGABRT" "$OUT/logcat.txt" | grep -q -i -E "cadviewer|cadcraft|$PKG"; then
  echo "::error::Panikk eller krasj i logcat"; fail=1
fi
if ! grep -q "lest «apartment-layout.dwg»" "$OUT/logcat.txt"; then
  echo "::error::Tegningen ble ikke lest"; fail=1
fi
if ! grep -q -E "bilde [0-9]+x[0-9]+ tegnet" "$OUT/logcat.txt"; then
  echo "::error::Tegningen ble ikke tegnet"; fail=1
fi
if grep -q "JNI-kall feilet" "$OUT/logcat.txt"; then
  echo "::error::Et JNI-kall feilet"; fail=1
fi
[ "$fail" = 0 ] || exit 1
echo "Røyktest OK"
