package dev.socketsweep;

/** Just enough JSON string escaping to avoid pulling in a library. */
final class Json {
    private Json() {}

    /** Appends {@code value} as a quoted JSON string. */
    static void appendString(StringBuilder sb, String value) {
        sb.append('"');
        for (int i = 0; i < value.length(); i++) {
            char c = value.charAt(i);
            switch (c) {
                case '"':
                    sb.append("\\\"");
                    break;
                case '\\':
                    sb.append("\\\\");
                    break;
                case '\n':
                    sb.append("\\n");
                    break;
                case '\r':
                    sb.append("\\r");
                    break;
                case '\t':
                    sb.append("\\t");
                    break;
                default:
                    // Other control characters must be escaped; everything else
                    // (including non-ASCII) passes through as UTF-8. Lone
                    // surrogates are replaced by the UTF-8 encoder.
                    if (c < 0x20) {
                        sb.append(String.format("\\u%04x", (int) c));
                    } else {
                        sb.append(c);
                    }
            }
        }
        sb.append('"');
    }
}
