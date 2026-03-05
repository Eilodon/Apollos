#include <jni.h>
#include <cstdint>

extern "C" {

struct ApollosKinematicOutput {
    float risk_score;
    uint8_t should_capture;
    float yaw_delta_deg;
};

uint32_t apollos_abi_version_u32(void);
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

} // extern "C"
