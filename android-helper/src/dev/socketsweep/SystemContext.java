package dev.socketsweep;

import android.content.Context;
import android.os.Looper;
import java.lang.reflect.Method;

/**
 * Obtains a framework {@link Context} from a bare {@code app_process}, where
 * no Application exists to ask. Uses the hidden {@code ActivityThread}, the
 * same route scrcpy takes. Hidden-API restrictions apply to app processes
 * forked from zygote, not to a process the shell starts directly.
 */
final class SystemContext {
    private SystemContext() {}

    static Context get() throws ReflectiveOperationException {
        prepareMainLooper();
        Class<?> activityThread = Class.forName("android.app.ActivityThread");
        Object thread = activityThread.getMethod("systemMain").invoke(null);
        Method getSystemContext = activityThread.getMethod("getSystemContext");
        // The system context belongs to the calling user (user 0 for the shell),
        // so package queries through it never touch other profiles.
        return (Context) getSystemContext.invoke(thread);
    }

    @SuppressWarnings("deprecation")
    private static void prepareMainLooper() {
        // ActivityThread insists on a main looper, which a plain process lacks.
        // Preparing twice throws, which just means one already exists.
        try {
            Looper.prepareMainLooper();
        } catch (RuntimeException ignored) {
            // already prepared
        }
    }
}
