#include <jni.h>
#include <algorithm>
#include <cstdint>
#include <vector>

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
    uint8_t distance_code;
};

struct ApollosEskfSnapshot {
    float sensor_health_score;
    uint8_t degraded;
    float localization_uncertainty_m;
    float innovation_norm;
    float covariance_xx;
    float covariance_yy;
    float covariance_zz;
};

struct ApollosBoundingBox {
    uint32_t label_id;
    float x_min;
    float y_min;
    float x_max;
    float y_max;
    float confidence;
};

struct ApollosDepthSpatials {
    float median_depth_m;
    float min_depth_m;
};

struct ApollosObjectSensorFusionInput {
    ApollosBoundingBox bbox;
    ApollosDepthSpatials spatial;
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
ApollosDepthHazardOutput apollos_detect_drop_ahead_objects(
    const ApollosObjectSensorFusionInput* objects_ptr,
    uintptr_t objects_len,
    float risk_score,
    uint8_t carry_mode_code,
    float gyro_magnitude,
    uint64_t now_ms
);
uint64_t apollos_eskf_create(void);
uint8_t apollos_eskf_destroy(uint64_t handle);
uint8_t apollos_eskf_reset(uint64_t handle);
uint8_t apollos_eskf_predict_imu(
    uint64_t handle,
    float accel_x,
    float accel_y,
    float accel_z,
    float dt_s
);
uint8_t apollos_eskf_update_vision(
    uint64_t handle,
    float position_x,
    float position_y,
    float position_z,
    float variance_m2
);
ApollosEskfSnapshot apollos_eskf_snapshot(uint64_t handle);

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
Java_com_apollos_nativeapp_RustCoreBridge_nativeDetectDropAheadObjects(
    JNIEnv* env,
    jobject /* thiz */,
    jfloatArray object_vector,
    jint object_count,
    jfloat risk_score,
    jbyte carry_mode_code,
    jfloat gyro_magnitude,
    jlong now_ms
) {
    constexpr int kObjectStride = 8;
    jsize vector_len = object_vector == nullptr ? 0 : env->GetArrayLength(object_vector);
    std::vector<jfloat> packed;
    if (vector_len > 0) {
        packed.resize(static_cast<size_t>(vector_len));
        env->GetFloatArrayRegion(object_vector, 0, vector_len, packed.data());
    }

    const int available = static_cast<int>(packed.size() / kObjectStride);
    const int requested = std::max(0, static_cast<int>(object_count));
    const int count = std::min(available, requested);

    std::vector<ApollosObjectSensorFusionInput> objects(static_cast<size_t>(count));
    for (int idx = 0; idx < count; ++idx) {
        const int base = idx * kObjectStride;
        objects[idx].bbox.label_id = static_cast<uint32_t>(std::max(0.0f, packed[base]));
        objects[idx].bbox.x_min = packed[base + 1];
        objects[idx].bbox.y_min = packed[base + 2];
        objects[idx].bbox.x_max = packed[base + 3];
        objects[idx].bbox.y_max = packed[base + 4];
        objects[idx].bbox.confidence = packed[base + 5];
        objects[idx].spatial.median_depth_m = packed[base + 6];
        objects[idx].spatial.min_depth_m = packed[base + 7];
    }

    const ApollosObjectSensorFusionInput* ptr = objects.empty() ? nullptr : objects.data();
    ApollosDepthHazardOutput output = apollos_detect_drop_ahead_objects(
        ptr,
        static_cast<uintptr_t>(objects.size()),
        static_cast<float>(risk_score),
        static_cast<uint8_t>(carry_mode_code),
        static_cast<float>(gyro_magnitude),
        static_cast<uint64_t>(now_ms)
    );

    jfloat values[4] = {
        output.detected == 0 ? 0.0f : 1.0f,
        output.position_x,
        output.confidence,
        static_cast<float>(output.distance_code),
    };

    jfloatArray array = env->NewFloatArray(4);
    if (array == nullptr) {
        return nullptr;
    }
    env->SetFloatArrayRegion(array, 0, 4, values);
    return array;
}

JNIEXPORT jlong JNICALL
Java_com_apollos_nativeapp_RustCoreBridge_nativeEskfCreate(
    JNIEnv* /* env */,
    jobject /* thiz */
) {
    return static_cast<jlong>(apollos_eskf_create());
}

JNIEXPORT jint JNICALL
Java_com_apollos_nativeapp_RustCoreBridge_nativeEskfDestroy(
    JNIEnv* /* env */,
    jobject /* thiz */,
    jlong handle
) {
    return static_cast<jint>(apollos_eskf_destroy(static_cast<uint64_t>(handle)));
}

JNIEXPORT jint JNICALL
Java_com_apollos_nativeapp_RustCoreBridge_nativeEskfReset(
    JNIEnv* /* env */,
    jobject /* thiz */,
    jlong handle
) {
    return static_cast<jint>(apollos_eskf_reset(static_cast<uint64_t>(handle)));
}

JNIEXPORT jint JNICALL
Java_com_apollos_nativeapp_RustCoreBridge_nativeEskfPredictImu(
    JNIEnv* /* env */,
    jobject /* thiz */,
    jlong handle,
    jfloat accel_x,
    jfloat accel_y,
    jfloat accel_z,
    jfloat dt_s
) {
    return static_cast<jint>(apollos_eskf_predict_imu(
        static_cast<uint64_t>(handle),
        static_cast<float>(accel_x),
        static_cast<float>(accel_y),
        static_cast<float>(accel_z),
        static_cast<float>(dt_s)
    ));
}

JNIEXPORT jint JNICALL
Java_com_apollos_nativeapp_RustCoreBridge_nativeEskfUpdateVision(
    JNIEnv* /* env */,
    jobject /* thiz */,
    jlong handle,
    jfloat position_x,
    jfloat position_y,
    jfloat position_z,
    jfloat variance_m2
) {
    return static_cast<jint>(apollos_eskf_update_vision(
        static_cast<uint64_t>(handle),
        static_cast<float>(position_x),
        static_cast<float>(position_y),
        static_cast<float>(position_z),
        static_cast<float>(variance_m2)
    ));
}


JNIEXPORT jfloatArray JNICALL
Java_com_apollos_nativeapp_RustCoreBridge_nativeEskfSnapshot(
    JNIEnv* env,
    jobject /* thiz */,
    jlong handle
) {
    ApollosEskfSnapshot snapshot = apollos_eskf_snapshot(static_cast<uint64_t>(handle));
    jfloat values[7] = {
        snapshot.sensor_health_score,
        snapshot.degraded == 0 ? 0.0f : 1.0f,
        snapshot.localization_uncertainty_m,
        snapshot.innovation_norm,
        snapshot.covariance_xx,
        snapshot.covariance_yy,
        snapshot.covariance_zz,
    };

    jfloatArray array = env->NewFloatArray(7);
    if (array == nullptr) {
        return nullptr;
    }
    env->SetFloatArrayRegion(array, 0, 7, values);
    return array;
}

} // extern "C"
