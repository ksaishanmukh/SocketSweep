package dev.socketsweep;

import java.util.HashSet;
import java.util.Set;

/** Parsed command line of the {@code apps} command. */
final class Options {
    private static final int DEFAULT_ICON_SIZE = 48;
    // Bounds keep a typo from allocating a huge bitmap per app.
    private static final int MIN_ICON_SIZE = 16;
    private static final int MAX_ICON_SIZE = 512;

    final Set<String> iconPackages = new HashSet<>();
    int iconSize = DEFAULT_ICON_SIZE;

    /** Returns null when the arguments are not understood. */
    static Options parse(String[] args) {
        if (args.length == 0 || !"apps".equals(args[0])) {
            return null;
        }
        Options options = new Options();
        for (int i = 1; i < args.length; i++) {
            if ("--icons".equals(args[i]) && i + 1 < args.length) {
                for (String pkg : args[++i].split(",")) {
                    if (!pkg.isEmpty()) {
                        options.iconPackages.add(pkg);
                    }
                }
            } else if ("--icon-size".equals(args[i]) && i + 1 < args.length) {
                try {
                    int size = Integer.parseInt(args[++i]);
                    options.iconSize = Math.max(MIN_ICON_SIZE, Math.min(MAX_ICON_SIZE, size));
                } catch (NumberFormatException e) {
                    return null;
                }
            } else {
                return null;
            }
        }
        return options;
    }
}
