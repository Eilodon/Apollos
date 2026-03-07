#ifndef APOLLOS_CORE_FFI_H
#define APOLLOS_CORE_FFI_H

#include <stdint.h>

typedef struct {
    float risk_score;
    uint8_t should_capture;
    float yaw_delta_deg;
} ApollosKinematicOutput;

typedef struct {
    uint8_t detected;
    float position_x;
    float confidence;
    uint8_t distance_code;
} ApollosDepthHazardOutput;

typedef struct {
    float sensor_health_score;
    uint8_t degraded;
    float localization_uncertainty_m;
    float innovation_norm;
    float covariance_xx;
    float covariance_yy;
    float covariance_zz;
} ApollosEskfSnapshot;

typedef struct {
    uint32_t label_id;
    float x_min;
    float y_min;
    float x_max;
    float y_max;
    float confidence;
} ApollosBoundingBox;

typedef struct {
    float median_depth_m;
    float min_depth_m;
} ApollosDepthSpatials;

typedef struct {
    ApollosBoundingBox bbox;
    ApollosDepthSpatials spatial;
} ApollosObjectSensorFusionInput;

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
uint8_t apollos_depth_onnx_runtime_enabled(void);
ApollosDepthHazardOutput apollos_detect_drop_ahead_objects(
    const ApollosObjectSensorFusionInput *objects_ptr,
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

#endif
