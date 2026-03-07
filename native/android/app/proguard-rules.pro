# Bảo vệ các phương thức JNI
-keepclasseswithmembernames class * {
    native <methods>;
}

# Bảo vệ toàn bộ Bridge giao tiếp với Rust
-keep class com.apollos.nativeapp.RustCoreBridge { *; }

# Bảo vệ các Data classes dùng cho Websocket/API
-keep class com.apollos.nativeapp.models.** { *; }
-keep class com.apollos.nativeapp.LocationSnapshot { *; }
-keep class com.apollos.nativeapp.TranscriptEntry { *; }
-keep class com.apollos.nativeapp.KinematicResult { *; }
-keep class com.apollos.nativeapp.DepthHazardResult { *; }
-keep class com.apollos.nativeapp.EdgeObjectDetection { *; }
-keep class com.apollos.nativeapp.EskfSnapshot { *; }
-keep class com.apollos.nativeapp.VisionOdometryResult { *; }

# TensorFlow Lite GPU delegates can reference backend option classes that are
# not packaged on every build target. Suppress these optional references so
# release minification can complete.
-dontwarn org.tensorflow.lite.gpu.GpuDelegateFactory$Options$GpuBackend
-dontwarn org.tensorflow.lite.gpu.GpuDelegateFactory$Options
