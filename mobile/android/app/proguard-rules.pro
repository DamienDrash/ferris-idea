# JNA and the UniFFI bindings find classes, fields and callbacks by reflection.
-keep class com.sun.jna.** { *; }
-keep class * implements com.sun.jna.** { *; }
-keep class uniffi.ferris_idea_mobile.** { *; }
-dontwarn java.awt.**
