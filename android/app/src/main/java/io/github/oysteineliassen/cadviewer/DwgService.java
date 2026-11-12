package io.github.oysteineliassen.cadviewer;

import android.app.Service;
import android.content.Intent;
import android.os.Bundle;
import android.os.Handler;
import android.os.IBinder;
import android.os.Looper;
import android.os.Message;
import android.os.Messenger;
import android.os.Process;
import android.os.RemoteException;
import android.util.Log;

/**
 * Gjør DWG → DXF i en egen prosess (android:process=":dwg"), med Rust-koden i
 * libcadcraft_viewer.so. DWG-lesingen er det mest minnekrevende steget: i en egen prosess frigis
 * minnet så snart den er ferdig, og hvis Android stopper prosessen fordi minnet tar slutt, får
 * appen beskjed (ServiceConnection) i stedet for å bli stoppet selv.
 */
public class DwgService extends Service {
    static {
        System.loadLibrary("cadcraft_viewer");
    }

    static final String TAG = "CadViewer";
    static final int MSG_CONVERT = 1;
    static final int MSG_DONE = 2;

    /** DWG `input` → DXF `output`. null ved suksess, ellers feilmeldingen. */
    private static native String nativeConvert(String input, String output);

    private final Messenger messenger = new Messenger(new Handler(Looper.getMainLooper(), msg -> {
        if (msg.what != MSG_CONVERT) return false;
        final String input = msg.getData().getString("input");
        final String output = msg.getData().getString("output");
        final Messenger reply = msg.replyTo;
        Log.i(TAG, "DwgService: konverterer " + input);
        // Egen tråd med stor stakk: DWG-leseren kan gå dypt i nestede strukturer.
        new Thread(null, () -> {
            String error = nativeConvert(input, output);
            Message done = Message.obtain(null, MSG_DONE);
            Bundle b = new Bundle();
            b.putString("error", error);
            done.setData(b);
            try {
                reply.send(done);
            } catch (RemoteException e) {
                Log.w(TAG, "DwgService: kunne ikke svare", e);
            }
        }, "dwg", 512L << 20).start();
        return true;
    }));

    @Override
    public IBinder onBind(Intent intent) {
        return messenger.getBinder();
    }

    @Override
    public boolean onUnbind(Intent intent) {
        // Ferdig: avslutt prosessen så minnet DWG-leseren brukte, gis tilbake med en gang.
        new Handler(Looper.getMainLooper()).postDelayed(() -> Process.killProcess(Process.myPid()), 300);
        return false;
    }
}
