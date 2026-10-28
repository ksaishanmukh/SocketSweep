package dev.socketsweep;

import android.content.pm.ApplicationInfo;
import android.content.pm.PackageManager;
import android.graphics.Bitmap;
import android.graphics.Canvas;
import android.graphics.drawable.Drawable;
import android.util.Base64;
import java.io.ByteArrayOutputStream;

/** Renders an app icon to a square PNG. */
final class IconRenderer {
    private IconRenderer() {}

    /** Base64 (no wrapping) PNG of the icon, or null when it cannot be drawn. */
    static String pngBase64(PackageManager pm, ApplicationInfo app, int size) {
        try {
            Drawable icon = app.loadIcon(pm);
            if (icon == null) {
                return null;
            }
            Bitmap bitmap = Bitmap.createBitmap(size, size, Bitmap.Config.ARGB_8888);
            // Drawing through a canvas, rather than reading a BitmapDrawable's
            // bitmap, also covers AdaptiveIconDrawable (layered, masked) and
            // vector icons, and scales everything to the requested size.
            Canvas canvas = new Canvas(bitmap);
            icon.setBounds(0, 0, size, size);
            icon.draw(canvas);
            ByteArrayOutputStream png = new ByteArrayOutputStream();
            bitmap.compress(Bitmap.CompressFormat.PNG, 100, png);
            bitmap.recycle();
            return Base64.encodeToString(png.toByteArray(), Base64.NO_WRAP);
        } catch (RuntimeException e) {
            // One unreadable icon should not abort the listing; stderr keeps the
            // reason visible without touching the stdout contract.
            System.err.println("socketsweep-helper: no icon for " + app.packageName + ": " + e);
            return null;
        }
    }
}
