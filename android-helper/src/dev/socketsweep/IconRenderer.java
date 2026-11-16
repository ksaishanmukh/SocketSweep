package dev.socketsweep;

import android.content.pm.ApplicationInfo;
import android.content.pm.PackageManager;
import android.content.res.Resources;
import android.graphics.Bitmap;
import android.graphics.BitmapFactory;
import android.graphics.Canvas;
import android.graphics.drawable.Drawable;
import android.util.Base64;
import android.util.TypedValue;
import java.io.ByteArrayOutputStream;
import java.io.IOException;
import java.io.InputStream;
import java.util.Locale;

/**
 * Renders an app icon to a square PNG straight from the app's own resources.
 *
 * <p>{@code ApplicationInfo.loadIcon} is avoided on purpose: vendor builds route
 * it through a theming layer (ColorOS builds a {@code Path} from a null native
 * handle) that crashes this bare process with SIGSEGV, which no try/catch can
 * survive.
 */
final class IconRenderer {
    private IconRenderer() {}

    /** Base64 (no wrapping) PNG of the icon, or null when it cannot be produced. */
    static String pngBase64(PackageManager pm, ApplicationInfo app, int size) {
        try {
            Bitmap bitmap = render(pm.getResourcesForApplication(app), app.icon, size);
            if (bitmap == null) {
                return null;
            }
            ByteArrayOutputStream png = new ByteArrayOutputStream();
            bitmap.compress(Bitmap.CompressFormat.PNG, 100, png);
            bitmap.recycle();
            return Base64.encodeToString(png.toByteArray(), Base64.NO_WRAP);
        } catch (Throwable t) {
            // One unreadable icon should not abort the listing; stderr keeps the
            // reason visible without touching the stdout contract.
            System.err.println("socketsweep-helper: no icon for " + app.packageName + ": " + t);
            return null;
        }
    }

    private static Bitmap render(Resources res, int id, int size) throws IOException {
        TypedValue value = new TypedValue();
        res.getValue(id, value, true);
        String path = value.string == null ? "" : value.string.toString().toLowerCase(Locale.ROOT);
        if (path.endsWith(".png") || path.endsWith(".webp") || path.endsWith(".jpg")) {
            return decodeBitmap(res, id, value, size);
        }
        // Adaptive and vector icons are XML; only those need the drawing path,
        // which also layers and masks them to the requested size.
        return drawDrawable(res.getDrawable(id, null), size);
    }

    private static Bitmap decodeBitmap(Resources res, int id, TypedValue value, int size)
            throws IOException {
        try (InputStream in = res.openRawResource(id, value)) {
            Bitmap source = BitmapFactory.decodeStream(in);
            if (source == null) {
                return null;
            }
            if (source.getWidth() == size && source.getHeight() == size) {
                return source;
            }
            Bitmap scaled = Bitmap.createScaledBitmap(source, size, size, true);
            if (scaled != source) {
                source.recycle();
            }
            return scaled;
        }
    }

    private static Bitmap drawDrawable(Drawable icon, int size) {
        if (icon == null) {
            return null;
        }
        // Software ARGB_8888, never a hardware bitmap, so the canvas stays on the CPU.
        Bitmap bitmap = Bitmap.createBitmap(size, size, Bitmap.Config.ARGB_8888);
        icon.setBounds(0, 0, size, size);
        icon.draw(new Canvas(bitmap));
        return bitmap;
    }
}
