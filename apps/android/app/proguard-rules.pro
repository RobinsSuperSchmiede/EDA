# JNI entry points (libopenshell_android.so calls nothing back, but the natives must keep their names)
-keep class dev.openshell.android.NativeRuntime { *; }
# Methods called from the launcher page
-keepclassmembers class * {
    @android.webkit.JavascriptInterface <methods>;
}
