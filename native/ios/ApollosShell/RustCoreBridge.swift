import Foundation

struct IOSKinematicResult {
    let riskScore: Float
    let shouldCapture: Bool
    let yawDeltaDeg: Float
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
}
