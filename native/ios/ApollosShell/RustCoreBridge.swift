import Foundation

struct IOSKinematicResult {
    let riskScore: Float
    let shouldCapture: Bool
    let yawDeltaDeg: Float
}

struct IOSDepthHazardResult {
    let detected: Bool
    let positionX: Float
    let confidence: Float
    let sourceCode: UInt8
    let distanceCode: UInt8
}

struct IOSEskfSnapshot {
    let sensorHealthScore: Float
    let degraded: Bool
    let localizationUncertaintyM: Float
    let innovationNorm: Float
    let covarianceXx: Float
    let covarianceYy: Float
    let covarianceZz: Float
}

struct IOSVisionOdometryResult {
    let applied: Bool
    let deltaXM: Float
    let deltaYM: Float
    let poseXM: Float
    let poseYM: Float
    let varianceM2: Float
    let opticalFlowScore: Float
    let lateralBias: Float
}

enum RustCoreBridge {
    static func abiVersionHex() -> String {
        let value = apollos_abi_version_u32()
        return String(format: "0x%08X", value)
    }

    static func analyzeDefaultWalkingFrame() -> IOSKinematicResult {
        let output = apollos_analyze_kinematics(
            2,
            1,
            11.0,
            2.0,
            6.0,
            0.0,
            9.8,
            0.2,
            4.0,
            2.0,
            1.0,
            0
        )

        return IOSKinematicResult(
            riskScore: output.risk_score,
            shouldCapture: output.should_capture != 0,
            yawDeltaDeg: output.yaw_delta_deg
        )
    }

    static func depthOnnxEnabled() -> Bool {
        apollos_depth_onnx_runtime_enabled() != 0
    }

    static func eskfCreate() -> UInt64 {
        apollos_eskf_create()
    }

    @discardableResult
    static func eskfDestroy(handle: UInt64) -> Bool {
        if handle == 0 {
            return false
        }
        return apollos_eskf_destroy(handle) != 0
    }

    @discardableResult
    static func eskfReset(handle: UInt64) -> Bool {
        if handle == 0 {
            return false
        }
        return apollos_eskf_reset(handle) != 0
    }

    @discardableResult
    static func eskfPredictImu(handle: UInt64, accelX: Float, accelY: Float, accelZ: Float, dtS: Float) -> Bool {
        if handle == 0 {
            return false
        }
        return apollos_eskf_predict_imu(handle, accelX, accelY, accelZ, dtS) != 0
    }

    @discardableResult
    static func eskfUpdateVision(
        handle: UInt64,
        positionX: Float,
        positionY: Float,
        positionZ: Float,
        varianceM2: Float
    ) -> Bool {
        if handle == 0 {
            return false
        }
        return apollos_eskf_update_vision(handle, positionX, positionY, positionZ, varianceM2) != 0
    }

    static func eskfSnapshot(handle: UInt64) -> IOSEskfSnapshot {
        if handle == 0 {
            return IOSEskfSnapshot(
                sensorHealthScore: 0.0,
                degraded: true,
                localizationUncertaintyM: 999.0,
                innovationNorm: 10.0,
                covarianceXx: 999.0,
                covarianceYy: 999.0,
                covarianceZz: 999.0
            )
        }

        let snapshot = apollos_eskf_snapshot(handle)
        return IOSEskfSnapshot(
            sensorHealthScore: snapshot.sensor_health_score,
            degraded: snapshot.degraded != 0,
            localizationUncertaintyM: snapshot.localization_uncertainty_m,
            innovationNorm: snapshot.innovation_norm,
            covarianceXx: snapshot.covariance_xx,
            covarianceYy: snapshot.covariance_yy,
            covarianceZz: snapshot.covariance_zz
        )
    }

    static func eskfUpdateVisualOdometryRgba(
        handle: UInt64,
        rgba: [UInt8],
        width: UInt32,
        height: UInt32,
        dtS: Float
    ) -> IOSVisionOdometryResult {
        if handle == 0 || width == 0 || height == 0 || dtS <= 0 {
            return IOSVisionOdometryResult(
                applied: false,
                deltaXM: 0.0,
                deltaYM: 0.0,
                poseXM: 0.0,
                poseYM: 0.0,
                varianceM2: 999.0,
                opticalFlowScore: 0.0,
                lateralBias: 0.0
            )
        }

        let output = rgba.withUnsafeBufferPointer { buffer -> ApollosVisionOdometryOutput in
            apollos_eskf_update_visual_odometry_rgba(
                handle,
                buffer.baseAddress,
                rgba.count,
                width,
                height,
                dtS
            )
        }

        return IOSVisionOdometryResult(
            applied: output.applied != 0,
            deltaXM: output.delta_x_m,
            deltaYM: output.delta_y_m,
            poseXM: output.pose_x_m,
            poseYM: output.pose_y_m,
            varianceM2: output.variance_m2,
            opticalFlowScore: output.optical_flow_score,
            lateralBias: output.lateral_bias
        )
    }

    static func eskfUpdateVisualOdometryBgraStrided(
        handle: UInt64,
        baseAddress: UnsafeRawPointer,
        bufferLen: Int,
        width: UInt32,
        height: UInt32,
        rowStride: UInt32,
        pixelStride: UInt32,
        dtS: Float
    ) -> IOSVisionOdometryResult {
        if handle == 0 || width == 0 || height == 0 || dtS <= 0 || bufferLen <= 0 {
            return IOSVisionOdometryResult(
                applied: false,
                deltaXM: 0.0,
                deltaYM: 0.0,
                poseXM: 0.0,
                poseYM: 0.0,
                varianceM2: 999.0,
                opticalFlowScore: 0.0,
                lateralBias: 0.0
            )
        }

        let output = apollos_eskf_update_visual_odometry_bgra_strided(
            handle,
            baseAddress.assumingMemoryBound(to: UInt8.self),
            bufferLen,
            width,
            height,
            rowStride,
            pixelStride,
            dtS
        )

        return IOSVisionOdometryResult(
            applied: output.applied != 0,
            deltaXM: output.delta_x_m,
            deltaYM: output.delta_y_m,
            poseXM: output.pose_x_m,
            poseYM: output.pose_y_m,
            varianceM2: output.variance_m2,
            opticalFlowScore: output.optical_flow_score,
            lateralBias: output.lateral_bias
        )
    }

    static func detectDropAheadRgba(
        rgba: [UInt8],
        width: UInt32,
        height: UInt32,
        riskScore: Float,
        carryModeCode: UInt8,
        gyroMagnitude: Float,
        nowMs: UInt64
    ) -> IOSDepthHazardResult {
        let output = rgba.withUnsafeBufferPointer { buffer -> ApollosDepthHazardOutput in
            apollos_detect_drop_ahead_rgba(
                buffer.baseAddress,
                rgba.count,
                width,
                height,
                riskScore,
                carryModeCode,
                gyroMagnitude,
                nowMs
            )
        }

        return IOSDepthHazardResult(
            detected: output.detected != 0,
            positionX: output.position_x,
            confidence: output.confidence,
            sourceCode: output.source_code,
            distanceCode: output.distance_code
        )
    }

    static func detectDropAheadBgraStrided(
        baseAddress: UnsafeRawPointer,
        bufferLen: Int,
        width: UInt32,
        height: UInt32,
        rowStride: UInt32,
        pixelStride: UInt32,
        riskScore: Float,
        carryModeCode: UInt8,
        gyroMagnitude: Float,
        nowMs: UInt64
    ) -> IOSDepthHazardResult {
        if width == 0 || height == 0 || bufferLen <= 0 {
            return IOSDepthHazardResult(
                detected: false,
                positionX: 0.0,
                confidence: 0.0,
                sourceCode: 0,
                distanceCode: 0
            )
        }

        let output = apollos_detect_drop_ahead_bgra_strided(
            baseAddress.assumingMemoryBound(to: UInt8.self),
            bufferLen,
            width,
            height,
            rowStride,
            pixelStride,
            riskScore,
            carryModeCode,
            gyroMagnitude,
            nowMs
        )
        return IOSDepthHazardResult(
            detected: output.detected != 0,
            positionX: output.position_x,
            confidence: output.confidence,
            sourceCode: output.source_code,
            distanceCode: output.distance_code
        )
    }
}
