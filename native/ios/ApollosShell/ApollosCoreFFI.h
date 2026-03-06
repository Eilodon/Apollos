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
    uint8_t source_code;
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
    uint8_t applied;
    float delta_x_m;
    float delta_y_m;
    float pose_x_m;
    float pose_y_m;
    float variance_m2;
    float optical_flow_score;
    float lateral_bias;
} ApollosVisionOdometryOutput;

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
ApollosDepthHazardOutput apollos_detect_drop_ahead_rgba(
    const uint8_t *rgba_ptr,
    uintptr_t rgba_len,
    uint32_t width,
    uint32_t height,
    float risk_score,
    uint8_t carry_mode_code,
    float gyro_magnitude,
    uint64_t now_ms
);
ApollosDepthHazardOutput apollos_detect_drop_ahead_rgba_strided(
    const uint8_t *rgba_ptr,
    uintptr_t rgba_len,
    uint32_t width,
    uint32_t height,
    uint32_t row_stride,
    uint32_t pixel_stride,
    float risk_score,
    uint8_t carry_mode_code,
    float gyro_magnitude,
    uint64_t now_ms
);
ApollosDepthHazardOutput apollos_detect_drop_ahead_bgra_strided(
    const uint8_t *bgra_ptr,
    uintptr_t bgra_len,
    uint32_t width,
    uint32_t height,
    uint32_t row_stride,
    uint32_t pixel_stride,
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
ApollosVisionOdometryOutput apollos_eskf_update_visual_odometry_rgba(
    uint64_t handle,
    const uint8_t *rgba_ptr,
    uintptr_t rgba_len,
    uint32_t width,
    uint32_t height,
    float dt_s
);
ApollosVisionOdometryOutput apollos_eskf_update_visual_odometry_rgba_strided(
    uint64_t handle,
    const uint8_t *rgba_ptr,
    uintptr_t rgba_len,
    uint32_t width,
    uint32_t height,
    uint32_t row_stride,
    uint32_t pixel_stride,
    float dt_s
);
ApollosVisionOdometryOutput apollos_eskf_update_visual_odometry_bgra_strided(
    uint64_t handle,
    const uint8_t *bgra_ptr,
    uintptr_t bgra_len,
    uint32_t width,
    uint32_t height,
    uint32_t row_stride,
    uint32_t pixel_stride,
    float dt_s
);
ApollosEskfSnapshot apollos_eskf_snapshot(uint64_t handle);

#endif
