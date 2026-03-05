/**
 * Kinematic Frame Gating - Thuật toán Toán học Gating Khung Hình
 *
 * Chỉ chụp ảnh gửi lên Cloud khi:
 * 1. Điện thoại đang ở trạng thái thẳng đứng (Gravity Vector gần song song trục Y)
 * 2. Không đang bị xoay vặn mạnh (Angular velocity thấp → không Motion Blur)
 *
 * Toán học: dùng Dot Product giữa vector gia tốc và vector trọng lực (0, 9.8, 0).
 * Khi điện thoại thẳng đứng, accel.y ≈ 9.8 và accel.z ≈ 0.
 */

export interface KinematicReading {
    accel: DeviceMotionEventAcceleration | null;
    gyro: DeviceMotionEventRotationRate | null;
}

export function computeRiskScore(
    motionState: 'stationary' | 'walking_slow' | 'walking_fast' | 'running',
    pitch: number,
    velocity: number,
    yawDeltaDeg: number,
): number {
    let score = 1;

    if (motionState === 'walking_fast') {
        score *= 1.5;
    } else if (motionState === 'running') {
        score *= 2;
    }

    if (Math.abs(pitch) > 20) {
        score *= 1.3;
    }

    if (Math.abs(yawDeltaDeg) > 30) {
        score *= 1.4;
    }

    if (velocity > 2.5 && Math.abs(pitch) > 15) {
        score *= 1.5;
    }

    return Math.min(4, Math.max(1, score));
}

/**
 * Kiểm tra xem có nên chụp khung hình gửi lên Cloud không.
 *
 * @param reading - dữ liệu gia tốc và con quay hồi chuyển từ DeviceMotion
 * @returns true nếu thiết bị thẳng đứng và ít bị lắc (OK để chụp)
 */
export function shouldCaptureFrame(reading: KinematicReading): boolean {
    const { accel, gyro } = reading;

    if (!accel || !gyro) {
        // Không có sensor → cho phép chụp bình thường (graceful degradation)
        return true;
    }

    const ax = accel.x ?? 0;
    const ay = accel.y ?? 0;
    const az = accel.z ?? 0;

    // Tính độ lớn vector gia tốc
    const magnitude = Math.sqrt(ax * ax + ay * ay + az * az);

    // Nếu không có trọng lực nào (free fall hoặc sensor lỗi) → cho phép chụp
    if (magnitude < 1.0) {
        return true;
    }

    // Góc giữa vector gia tốc và trục Y (thẳng đứng): cos(θ) = ay / |a|
    // θ ≈ 0 → điện thoại đứng thẳng.
    const cosTilt = Math.abs(ay) / magnitude;
    const isVertical = cosTilt > 0.82; // ≈ cos(35°) → cho phép nghiêng tới 35°

    // Angular stability: không được xoay quá 45°/s
    const alpha = gyro.alpha ?? 0;
    const beta = gyro.beta ?? 0;
    const gamma = gyro.gamma ?? 0;
    const isStable = Math.abs(alpha) < 45 && Math.abs(beta) < 45 && Math.abs(gamma) < 45;

    return isVertical && isStable;
}

/**
 * Tính góc xoay ngang (yaw delta) từ gyroscope để bơm vào Odometry context.
 * Trả về góc xoay tích lũy (radians/second từ gyro.alpha).
 */
export function computeYawDelta(gyro: DeviceMotionEventRotationRate | null, dtMs: number): number {
    if (!gyro || gyro.alpha === null) {
        return 0;
    }
    // alpha = deg/s → đổi sang degrees tại thời điểm này
    return (gyro.alpha * dtMs) / 1000;
}
