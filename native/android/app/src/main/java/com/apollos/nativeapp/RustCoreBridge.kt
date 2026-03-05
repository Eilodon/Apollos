package com.apollos.nativeapp

data class KinematicResult(
    val riskScore: Float,
    val shouldCapture: Boolean,
    val yawDeltaDeg: Float,
)

data class DepthHazardResult(
    val detected: Boolean,
    val positionX: Float,
    val confidence: Float,
    val sourceCode: Int,
    val distanceCode: Int,
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

    private external fun nativeDepthOnnxEnabled(): Int

    private external fun nativeDetectDropAheadRgba(
        rgbaBytes: ByteArray,
        width: Int,
        height: Int,
        riskScore: Float,
        carryModeCode: Byte,
        gyroMagnitude: Float,
        nowMs: Long,
    ): FloatArray

    fun abiVersion(): Int = nativeAbiVersion()
    fun depthOnnxEnabled(): Boolean = nativeDepthOnnxEnabled() != 0

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

    fun detectDropAheadRgba(
        rgbaBytes: ByteArray,
        width: Int,
        height: Int,
        riskScore: Float,
        carryModeCode: Byte,
        gyroMagnitude: Float,
        nowMs: Long,
    ): DepthHazardResult {
        val output = nativeDetectDropAheadRgba(
            rgbaBytes = rgbaBytes,
            width = width,
            height = height,
            riskScore = riskScore,
            carryModeCode = carryModeCode,
            gyroMagnitude = gyroMagnitude,
            nowMs = nowMs,
        )
        return DepthHazardResult(
            detected = output.getOrElse(0) { 0.0f } > 0.5f,
            positionX = output.getOrElse(1) { 0.0f },
            confidence = output.getOrElse(2) { 0.0f },
            sourceCode = output.getOrElse(3) { 0.0f }.toInt(),
            distanceCode = output.getOrElse(4) { 0.0f }.toInt(),
        )
    }
}
