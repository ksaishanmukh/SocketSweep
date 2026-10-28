package dev.socketsweep;

import android.content.pm.ApplicationInfo;
import android.content.pm.PackageManager;
import java.io.PrintStream;
import java.util.List;

/**
 * Entry point, run on the phone as the shell user through {@code app_process}:
 *
 * <pre>
 * CLASSPATH=/data/local/tmp/socketsweep-helper.dex app_process / \
 *     dev.socketsweep.Helper apps [--icons pkg1,pkg2] [--icon-size 48]
 * </pre>
 *
 * Output is UTF-8, one JSON object per line: {"v":1} first, then one object
 * per installed app. Diagnostics go to stderr and any failure exits non-zero,
 * so the host can tell an empty phone from a broken helper.
 */
public final class Helper {
    /** Bumped together with the Rust parser (socketsweep-wire's apps module). */
    private static final int FORMAT_VERSION = 1;

    private Helper() {}

    public static void main(String[] args) {
        int code;
        try {
            code = run(args);
        } catch (Throwable t) {
            // Reflection into hidden framework APIs can fail in many shapes; the
            // host only needs the text and a non-zero exit.
            System.err.println("socketsweep-helper: " + t);
            code = 1;
        }
        System.out.flush();
        // Skip shutdown hooks and the framework's lingering non-daemon threads.
        Runtime.getRuntime().halt(code);
    }

    private static int run(String[] args) throws Exception {
        Options options = Options.parse(args);
        if (options == null) {
            System.err.println("usage: Helper apps [--icons pkg1,pkg2,...] [--icon-size px]");
            return 2;
        }
        PackageManager pm = SystemContext.get().getPackageManager();
        List<ApplicationInfo> apps = AppLister.installed(pm);

        PrintStream out = new PrintStream(System.out, false, "UTF-8");
        out.print("{\"v\":" + FORMAT_VERSION + "}\n");
        for (ApplicationInfo app : apps) {
            out.print(AppLister.line(pm, app, options) + "\n");
        }
        out.flush();
        return 0;
    }
}
