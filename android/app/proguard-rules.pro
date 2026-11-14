# JNA + UniFFI generated bindings are reached via reflection/native callbacks.
-keep class com.sun.jna.** { *; }
-keep class * implements com.sun.jna.** { *; }
-keep class uniffi.** { *; }
-dontwarn java.awt.**
# libwebrtc: Java classes are created and called from native code (JNI).
-keep class org.webrtc.** { *; }
-dontwarn org.webrtc.**
# libwebrtc's JNI_OnLoad looks up JNI Zero classes (org.jni_zero) by name; without them the
# release build aborts as soon as a call starts its media ("java_class == null").
-keep class org.jni_zero.** { *; }
-dontwarn org.jni_zero.**
