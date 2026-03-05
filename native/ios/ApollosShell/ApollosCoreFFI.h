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

#endif
