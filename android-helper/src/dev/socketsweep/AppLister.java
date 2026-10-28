package dev.socketsweep;

import android.content.pm.ApplicationInfo;
import android.content.pm.PackageManager;
import android.os.Build;
import java.util.List;

/** Lists the current user's installed apps and formats them as output lines. */
final class AppLister {
    private AppLister() {}

    /**
     * Only the current user's apps. The shell may not query other users
     * (a work or clone profile), so a SecurityException propagates to the
     * caller's error path instead of being worked around.
     */
    @SuppressWarnings("deprecation")
    static List<ApplicationInfo> installed(PackageManager pm) {
        if (Build.VERSION.SDK_INT >= 33) {
            return pm.getInstalledApplications(PackageManager.ApplicationInfoFlags.of(0L));
        }
        return pm.getInstalledApplications(0);
    }

    static String line(PackageManager pm, ApplicationInfo app, Options options) {
        StringBuilder sb = new StringBuilder(128);
        sb.append("{\"pkg\":");
        Json.appendString(sb, app.packageName);
        sb.append(",\"label\":");
        Json.appendString(sb, label(pm, app));
        sb.append(",\"system\":").append(isSystem(app));
        if (options.iconPackages.contains(app.packageName)) {
            String icon = IconRenderer.pngBase64(pm, app, options.iconSize);
            if (icon != null) {
                sb.append(",\"icon\":\"").append(icon).append('"');
            }
        }
        return sb.append('}').toString();
    }

    private static String label(PackageManager pm, ApplicationInfo app) {
        try {
            CharSequence label = app.loadLabel(pm);
            return label == null ? app.packageName : label.toString();
        } catch (RuntimeException e) {
            // A broken resource table should cost one label, not the listing.
            return app.packageName;
        }
    }

    private static boolean isSystem(ApplicationInfo app) {
        int system = ApplicationInfo.FLAG_SYSTEM | ApplicationInfo.FLAG_UPDATED_SYSTEM_APP;
        return (app.flags & system) != 0;
    }
}
