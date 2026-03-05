#include <jni.h>
#include <cstdint>

extern "C" {

struct ApollosKinematicOutput {
    float risk_score;
    uint8_t should_capture;
    float yaw_delta_deg;
};

struct ApollosDepthHazardOutput {
    uint8_t detected;
    float position_x;
    float confidence;
    uint8_t source_code;
    uint8_t distance_code;
};

uint32_t apollos_abi_version_u32(void);
uint8_t apollos_depth_onnx_runtime_enabled(void);
ApollosKinematicOutput apollos_analyze_kinematics(
    uint8_t motion_state_code,
    uint8_t carry_mode_code,
    float pitch,
    float velocity,
    float yaw_delta_deg,
    float accel_x,
    float accel_y,
    float accel_z,
    float gyro_alpha,
    float gyro_beta,
    float gyro_gamma,
    uint8_t sensor_unavailable
);
ApollosDepthHazardOutput apollos_detect_drop_ahead_rgba(
    const uint8_t* rgba_ptr,
    uintptr_t rgba_len,
    uint32_t width,
    uint32_t height,
    float risk_score,
    uint8_t carry_mode_code,
    float gyro_magnitude,
    uint64_t now_ms
);

JNIEXPORT jint JNICALL
Java_com_apollos_nativeapp_RustCoreBridge_nativeAbiVersion(
    JNIEnv* /* env */,
    jobject /* thiz */
) {
    return static_cast<jint>(apollos_abi_version_u32());
}

JNIEXPORT jfloatArray JNICALL
Java_com_apollos_nativeapp_RustCoreBridge_nativeAnalyzeKinematics(
    JNIEnv* env,
    jobject /* thiz */,
    jbyte motion_state_code,
    jbyte carry_mode_code,
    jfloat pitch,
    jfloat velocity,
    jfloat yaw_delta_deg,
    jfloat accel_x,
    jfloat accel_y,
    jfloat accel_z,
    jfloat gyro_alpha,
    jfloat gyro_beta,
    jfloat gyro_gamma,
    jbyte sensor_unavailable
) {
    ApollosKinematicOutput output = apollos_analyze_kinematics(
        static_cast<uint8_t>(motion_state_code),
        static_cast<uint8_t>(carry_mode_code),
        static_cast<float>(pitch),
        static_cast<float>(velocity),
        static_cast<float>(yaw_delta_deg),
        static_cast<float>(accel_x),
        static_cast<float>(accel_y),
        static_cast<float>(accel_z),
        static_cast<float>(gyro_alpha),
        static_cast<float>(gyro_beta),
        static_cast<float>(gyro_gamma),
        static_cast<uint8_t>(sensor_unavailable)
    );

    jfloat values[3] = {
        output.risk_score,
        output.should_capture == 0 ? 0.0f : 1.0f,
        output.yaw_delta_deg,
    };

    jfloatArray array = env->NewFloatArray(3);
    if (array == nullptr) {
        return nullptr;
    }

    env->SetFloatArrayRegion(array, 0, 3, values);
    return array;
}

JNIEXPORT jint JNICALL
Java_com_apollos_nativeapp_RustCoreBridge_nativeDepthOnnxEnabled(
    JNIEnv* /* env */,
    jobject /* thiz */
) {
    return static_cast<jint>(apollos_depth_onnx_runtime_enabled());
}

JNIEXPORT jfloatArray JNICALL
Java_com_apollos_nativeapp_RustCoreBridge_nativeDetectDropAheadRgba(
    JNIEnv* env,
    jobject /* thiz */,
    jbyteArray rgba_bytes,
    jint width,
    jint height,
    jfloat risk_score,
    jbyte carry_mode_code,
    jfloat gyro_magnitude,
    jlong now_ms
) {
    if (rgba_bytes == nullptr || width <= 0 || height <= 0) {
        return nullptr;
    }

    const jsize len = env->GetArrayLength(rgba_bytes);
    jbyte* bytes = env->GetByteArrayElements(rgba_bytes, nullptr);
    if (bytes == nullptr) {
        return nullptr;
    }

    ApollosDepthHazardOutput output = apollos_detect_drop_ahead_rgba(
        reinterpret_cast<const uint8_t*>(bytes),
        static_cast<uintptr_t>(len),
        static_cast<uint32_t>(width),
        static_cast<uint32_t>(height),
        static_cast<float>(risk_score),
        static_cast<uint8_t>(carry_mode_code),
        static_cast<float>(gyro_magnitude),
        static_cast<uint64_t>(now_ms)
    );
    env->ReleaseByteArrayElements(rgba_bytes, bytes, JNI_ABORT);

    jfloat values[5] = {
        output.detected == 0 ? 0.0f : 1.0f,
        output.position_x,
        output.confidence,
        static_cast<float>(output.source_code),
        static_cast<float>(output.distance_code),
    };

    jfloatArray array = env->NewFloatArray(5);
    if (array == nullptr) {
        return nullptr;
    }
    env->SetFloatArrayRegion(array, 0, 5, values);
    return array;
}

} // extern "C"
