#ifndef APOLLOS_CORE_FFI_H
#define APOLLOS_CORE_FFI_H

#include <stdint.h>

typedef struct {
    float risk_score;
    uint8_t should_capture;
    float yaw_delta_deg;
} ApollosKinematicOutput;

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

#endif
