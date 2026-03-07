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

struct IOSEdgeObjectDetection {
    let labelId: UInt32
    let xMin: Float
    let yMin: Float
    let xMax: Float
    let yMax: Float
    let confidence: Float
    let medianDepthM: Float
    let minDepthM: Float
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


    static func detectDropAheadObjects(
        objects: [IOSEdgeObjectDetection],
        riskScore: Float,
        carryModeCode: UInt8,
        gyroMagnitude: Float,
        nowMs: UInt64
    ) -> IOSDepthHazardResult {
        if objects.isEmpty {
            return IOSDepthHazardResult(
                detected: false,
                positionX: 0.0,
                confidence: 0.0,
                distanceCode: 0
            )
        }

        let mapped = objects.prefix(32).map { obj in
            ApollosObjectSensorFusionInput(
                bbox: ApollosBoundingBox(
                    label_id: obj.labelId,
                    x_min: obj.xMin,
                    y_min: obj.yMin,
                    x_max: obj.xMax,
                    y_max: obj.yMax,
                    confidence: obj.confidence
                ),
                spatial: ApollosDepthSpatials(
                    median_depth_m: obj.medianDepthM,
                    min_depth_m: obj.minDepthM
                )
            )
        }

        let output = mapped.withUnsafeBufferPointer { buffer -> ApollosDepthHazardOutput in
            apollos_detect_drop_ahead_objects(
                buffer.baseAddress,
                mapped.count,
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
            distanceCode: output.distance_code
        )
    }
}
