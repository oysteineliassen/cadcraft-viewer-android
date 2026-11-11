package io.github.oysteineliassen.cadviewer;

import android.app.NativeActivity;
import android.content.ActivityNotFoundException;
import android.content.Intent;
import android.database.Cursor;
import android.graphics.Color;
import android.net.Uri;
import android.os.Build;
import android.os.Bundle;
import android.provider.OpenableColumns;
import android.util.Log;
import android.view.View;
import android.view.Window;
import android.view.WindowInsets;
import android.view.WindowInsetsController;
import android.widget.Toast;

import java.io.File;
import java.io.FileInputStream;
import java.io.FileOutputStream;
import java.io.IOException;
import java.io.InputStream;
import java.io.OutputStream;

/**
 * Tynn Java-del rundt Rust-appen (libcadcraft_viewer.so, startet av NativeActivity).
 *
 * Den gjør det som krever Android-API-er: tar imot filer fra «Åpne med» / «Del» og fra
 * filvelgeren, kopierer dem til appens cache og gir stien til Rust; lagrer eksportert PDF der
 * brukeren velger; og forteller Rust hvor stor statuslinjen og navigasjonslinjen er.
 */
public class MainActivity extends NativeActivity {
    static {
        // Laster biblioteket via System.loadLibrary også, så JNI finner native-metodene under.
        System.loadLibrary("cadcraft_viewer");
    }

    private static final String TAG = "CadViewer";
    private static final int REQUEST_OPEN = 1;
    private static final int REQUEST_SAVE_PDF = 2;
    /** Største fil som åpnes (byte). */
    private static final long MAX_BYTES = 1L << 30;

    private String pendingPdf;

    private native void nativeOpenFile(String path, String name);
    private native void nativeMessage(String text);
    private native void nativeInsets(int left, int top, int right, int bottom);

    @Override
    protected void onCreate(Bundle savedInstanceState) {
        Log.i(TAG, "onCreate");
        super.onCreate(savedInstanceState);
        if (savedInstanceState != null) {
            pendingPdf = savedInstanceState.getString("pendingPdf");
        }
        drawBehindSystemBars();
        handleIntent(getIntent());
    }

    @Override
    protected void onSaveInstanceState(Bundle out) {
        super.onSaveInstanceState(out);
        out.putString("pendingPdf", pendingPdf);
    }

    @Override
    protected void onNewIntent(Intent intent) {
        Log.i(TAG, "onNewIntent " + intent);
        super.onNewIntent(intent);
        setIntent(intent);
        handleIntent(intent);
    }

    /** Tegningsflaten fyller hele skjermen; Rust-appen holder knappene unna systemlinjene. */
    private void drawBehindSystemBars() {
        Window w = getWindow();
        w.setStatusBarColor(Color.TRANSPARENT);
        w.setNavigationBarColor(Color.TRANSPARENT);
        if (Build.VERSION.SDK_INT >= 30) {
            w.setDecorFitsSystemWindows(false);
        } else {
            w.getDecorView().setSystemUiVisibility(View.SYSTEM_UI_FLAG_LAYOUT_STABLE
                    | View.SYSTEM_UI_FLAG_LAYOUT_FULLSCREEN
                    | View.SYSTEM_UI_FLAG_LAYOUT_HIDE_NAVIGATION);
        }
        if (Build.VERSION.SDK_INT >= 28) {
            w.getAttributes().layoutInDisplayCutoutMode =
                    android.view.WindowManager.LayoutParams.LAYOUT_IN_DISPLAY_CUTOUT_MODE_SHORT_EDGES;
        }
        View decor = w.getDecorView();
        decor.setOnApplyWindowInsetsListener((v, insets) -> {
            int l, t, r, b;
            if (Build.VERSION.SDK_INT >= 30) {
                android.graphics.Insets i = insets.getInsets(
                        WindowInsets.Type.systemBars() | WindowInsets.Type.displayCutout());
                l = i.left; t = i.top; r = i.right; b = i.bottom;
            } else {
                l = insets.getSystemWindowInsetLeft();
                t = insets.getSystemWindowInsetTop();
                r = insets.getSystemWindowInsetRight();
                b = insets.getSystemWindowInsetBottom();
            }
            Log.i(TAG, "insets " + l + "," + t + "," + r + "," + b);
            nativeInsets(l, t, r, b);
            return v.onApplyWindowInsets(insets);
        });
        decor.requestApplyInsets();
    }

    /** Kalles fra Rust: mørke ikoner i systemlinjene på lys bakgrunn, lyse på mørk. */
    public void setLightBars(final boolean light) {
        Log.i(TAG, "setLightBars " + light);
        runOnUiThread(() -> {
            Window w = getWindow();
            if (Build.VERSION.SDK_INT >= 30) {
                WindowInsetsController c = w.getInsetsController();
                if (c != null) {
                    int mask = WindowInsetsController.APPEARANCE_LIGHT_STATUS_BARS
                            | WindowInsetsController.APPEARANCE_LIGHT_NAVIGATION_BARS;
                    c.setSystemBarsAppearance(light ? mask : 0, mask);
                }
            } else {
                View d = w.getDecorView();
                int flags = d.getSystemUiVisibility();
                int mask = View.SYSTEM_UI_FLAG_LIGHT_STATUS_BAR | View.SYSTEM_UI_FLAG_LIGHT_NAVIGATION_BAR;
                d.setSystemUiVisibility(light ? (flags | mask) : (flags & ~mask));
            }
        });
    }

    private void handleIntent(Intent intent) {
        if (intent == null) return;
        Uri uri = null;
        String action = intent.getAction();
        if (Intent.ACTION_VIEW.equals(action)) {
            uri = intent.getData();
        } else if (Intent.ACTION_SEND.equals(action)) {
            uri = intent.getParcelableExtra(Intent.EXTRA_STREAM);
        }
        Log.i(TAG, "handleIntent " + action + " " + uri);
        if (uri != null) {
            // Ikke åpne samme fil igjen når aktiviteten gjenskapes (f.eks. ved rotasjon).
            intent.setAction(Intent.ACTION_MAIN);
            intent.setData(null);
            copyAndOpen(uri);
        }
    }

    /** Kalles fra Rust: vis Androids filvelger. */
    public void openPicker() {
        Log.i(TAG, "openPicker");
        runOnUiThread(() -> {
            Intent i = new Intent(Intent.ACTION_OPEN_DOCUMENT);
            i.addCategory(Intent.CATEGORY_OPENABLE);
            // DWG/DXF har ingen fast MIME-type, så alle filer vises.
            i.setType("*/*");
            try {
                startActivityForResult(i, REQUEST_OPEN);
            } catch (ActivityNotFoundException e) {
                Intent g = new Intent(Intent.ACTION_GET_CONTENT);
                g.addCategory(Intent.CATEGORY_OPENABLE);
                g.setType("*/*");
                try {
                    startActivityForResult(Intent.createChooser(g, "Åpne tegning"), REQUEST_OPEN);
                } catch (ActivityNotFoundException e2) {
                    nativeMessage("Fant ingen filvelger på telefonen.");
                }
            }
        });
    }

    /** Kalles fra Rust: la brukeren velge hvor PDF-en (allerede skrevet til `path`) skal lagres. */
    public void savePdf(final String path, final String name) {
        runOnUiThread(() -> {
            pendingPdf = path;
            Intent i = new Intent(Intent.ACTION_CREATE_DOCUMENT);
            i.addCategory(Intent.CATEGORY_OPENABLE);
            i.setType("application/pdf");
            i.putExtra(Intent.EXTRA_TITLE, name);
            try {
                startActivityForResult(i, REQUEST_SAVE_PDF);
            } catch (ActivityNotFoundException e) {
                nativeMessage("Fant ingen app for å lagre filer.");
            }
        });
    }

    @Override
    protected void onActivityResult(int requestCode, int resultCode, Intent data) {
        super.onActivityResult(requestCode, resultCode, data);
        if (resultCode != RESULT_OK || data == null || data.getData() == null) return;
        Uri uri = data.getData();
        if (requestCode == REQUEST_OPEN) {
            copyAndOpen(uri);
        } else if (requestCode == REQUEST_SAVE_PDF && pendingPdf != null) {
            final String src = pendingPdf;
            pendingPdf = null;
            new Thread(() -> {
                try (InputStream in = new FileInputStream(src);
                     OutputStream out = getContentResolver().openOutputStream(uri, "wt")) {
                    if (out == null) throw new IOException("kunne ikke åpne målet");
                    copy(in, out, Long.MAX_VALUE);
                    toast("PDF lagret");
                } catch (Exception e) {
                    nativeMessage("Kunne ikke lagre PDF: " + e.getMessage());
                } finally {
                    new File(src).delete();
                }
            }, "lagre-pdf").start();
        }
    }

    /** Kopier filen bak `uri` til cachen (i bakgrunnen) og be Rust åpne kopien. */
    private void copyAndOpen(final Uri uri) {
        final String name = displayName(uri);
        new Thread(() -> {
            File dir = new File(getCacheDir(), "open");
            File[] old = dir.listFiles();
            if (old != null) for (File f : old) f.delete();
            if (!dir.isDirectory() && !dir.mkdirs()) {
                nativeMessage("Kunne ikke lage mappe for filen.");
                return;
            }
            File target = new File(dir, safeFileName(name));
            try (InputStream in = getContentResolver().openInputStream(uri);
                 OutputStream out = new FileOutputStream(target)) {
                if (in == null) throw new IOException("ingen data");
                copy(in, out, MAX_BYTES);
            } catch (Exception e) {
                target.delete();
                Log.w(TAG, "kopiering feilet", e);
                nativeMessage("Kunne ikke lese «" + name + "»: " + e.getMessage());
                return;
            }
            Log.i(TAG, "kopiert til " + target + " (" + target.length() + " byte)");
            nativeOpenFile(target.getAbsolutePath(), name);
        }, "kopier-fil").start();
    }

    private String displayName(Uri uri) {
        String name = null;
        if ("content".equals(uri.getScheme())) {
            try (Cursor c = getContentResolver().query(uri,
                    new String[]{OpenableColumns.DISPLAY_NAME}, null, null, null)) {
                if (c != null && c.moveToFirst()) {
                    int i = c.getColumnIndex(OpenableColumns.DISPLAY_NAME);
                    if (i >= 0) name = c.getString(i);
                }
            } catch (Exception ignored) {
            }
        }
        if (name == null || name.isEmpty()) name = uri.getLastPathSegment();
        if (name == null || name.isEmpty()) name = "tegning";
        return name;
    }

    private static String safeFileName(String name) {
        String s = name.replaceAll("[\\\\/:*?\"<>|\\x00-\\x1f]", "_");
        if (s.length() > 120) s = s.substring(s.length() - 120);
        return s.isEmpty() ? "tegning" : s;
    }

    private static void copy(InputStream in, OutputStream out, long max) throws IOException {
        byte[] buf = new byte[1 << 16];
        long total = 0;
        int n;
        while ((n = in.read(buf)) > 0) {
            total += n;
            if (total > max) throw new IOException("filen er for stor");
            out.write(buf, 0, n);
        }
    }

    private void toast(final String text) {
        runOnUiThread(() -> Toast.makeText(this, text, Toast.LENGTH_SHORT).show());
    }
}
