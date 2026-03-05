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
}
