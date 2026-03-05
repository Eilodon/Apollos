package com.apollos.nativeapp

data class KinematicResult(
    val riskScore: Float,
    val shouldCapture: Boolean,
    val yawDeltaDeg: Float,
)

object RustCoreBridge {
    init {
        System.loadLibrary("rust_bridge")
    }

    private external fun nativeAbiVersion(): Int

    private external fun nativeAnalyzeKinematics(
        motionStateCode: Byte,
        carryModeCode: Byte,
        pitch: Float,
        velocity: Float,
        yawDeltaDeg: Float,
        accelX: Float,
        accelY: Float,
        accelZ: Float,
        gyroAlpha: Float,
        gyroBeta: Float,
        gyroGamma: Float,
        sensorUnavailable: Byte,
    ): FloatArray

    fun abiVersion(): Int = nativeAbiVersion()

    fun analyzeDefaultWalkingFrame(): KinematicResult {
        val output = nativeAnalyzeKinematics(
            2,
            1,
            11.0f,
            2.0f,
            6.0f,
            0.0f,
            9.8f,
            0.2f,
            4.0f,
            2.0f,
            1.0f,
            0,
        )

        return KinematicResult(
            riskScore = output.getOrElse(0) { 0.0f },
            shouldCapture = output.getOrElse(1) { 0.0f } > 0.5f,
            yawDeltaDeg = output.getOrElse(2) { 0.0f },
        )
    }
}
