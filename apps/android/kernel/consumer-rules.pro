# JNA finds the bindings' structures and callbacks by reflection: keep them, and JNA itself.
-keep class com.sun.jna.** { *; }
-keep class * implements com.sun.jna.** { *; }
-keep class dev.keel.kernel.** { *; }
-dontwarn java.awt.**
