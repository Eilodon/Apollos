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

data class EskfSnapshot(
    val sensorHealthScore: Float,
    val degraded: Boolean,
    val localizationUncertaintyM: Float,
    val innovationNorm: Float,
    val covarianceXx: Float,
    val covarianceYy: Float,
    val covarianceZz: Float,
)

data class VisionOdometryResult(
    val applied: Boolean,
    val deltaXM: Float,
    val deltaYM: Float,
    val poseXM: Float,
    val poseYM: Float,
    val varianceM2: Float,
    val opticalFlowScore: Float,
    val lateralBias: Float,
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
    private external fun nativeEskfCreate(): Long
    private external fun nativeEskfDestroy(handle: Long): Int
    private external fun nativeEskfReset(handle: Long): Int
    private external fun nativeEskfPredictImu(
        handle: Long,
        accelX: Float,
        accelY: Float,
        accelZ: Float,
        dtS: Float,
    ): Int

    private external fun nativeEskfUpdateVision(
        handle: Long,
        positionX: Float,
        positionY: Float,
        positionZ: Float,
        varianceM2: Float,
    ): Int

    private external fun nativeEskfUpdateVisualOdometryRgba(
        handle: Long,
        rgbaBytes: ByteArray,
        width: Int,
        height: Int,
        dtS: Float,
    ): FloatArray

    private external fun nativeEskfSnapshot(handle: Long): FloatArray

    fun abiVersion(): Int = nativeAbiVersion()
    fun depthOnnxEnabled(): Boolean = nativeDepthOnnxEnabled() != 0
    fun eskfCreate(): Long = nativeEskfCreate()
    fun eskfDestroy(handle: Long): Boolean = handle != 0L && nativeEskfDestroy(handle) != 0
    fun eskfReset(handle: Long): Boolean = handle != 0L && nativeEskfReset(handle) != 0

    fun eskfPredictImu(handle: Long, accelX: Float, accelY: Float, accelZ: Float, dtS: Float): Boolean {
        if (handle == 0L) {
            return false
        }
        return nativeEskfPredictImu(handle, accelX, accelY, accelZ, dtS) != 0
    }

    fun eskfUpdateVision(
        handle: Long,
        positionX: Float,
        positionY: Float,
        positionZ: Float,
        varianceM2: Float,
    ): Boolean {
        if (handle == 0L) {
            return false
        }
        return nativeEskfUpdateVision(handle, positionX, positionY, positionZ, varianceM2) != 0
    }

    fun eskfSnapshot(handle: Long): EskfSnapshot {
        if (handle == 0L) {
            return EskfSnapshot(
                sensorHealthScore = 0.0f,
                degraded = true,
                localizationUncertaintyM = 999.0f,
                innovationNorm = 10.0f,
                covarianceXx = 999.0f,
                covarianceYy = 999.0f,
                covarianceZz = 999.0f,
            )
        }
        val output = nativeEskfSnapshot(handle)
        return EskfSnapshot(
            sensorHealthScore = output.getOrElse(0) { 0.0f },
            degraded = output.getOrElse(1) { 1.0f } > 0.5f,
            localizationUncertaintyM = output.getOrElse(2) { 999.0f },
            innovationNorm = output.getOrElse(3) { 10.0f },
            covarianceXx = output.getOrElse(4) { 999.0f },
            covarianceYy = output.getOrElse(5) { 999.0f },
            covarianceZz = output.getOrElse(6) { 999.0f },
        )
    }

    fun eskfUpdateVisualOdometryRgba(
        handle: Long,
        rgbaBytes: ByteArray,
        width: Int,
        height: Int,
        dtS: Float,
    ): VisionOdometryResult {
        if (handle == 0L || width <= 0 || height <= 0 || dtS <= 0f) {
            return VisionOdometryResult(
                applied = false,
                deltaXM = 0.0f,
                deltaYM = 0.0f,
                poseXM = 0.0f,
                poseYM = 0.0f,
                varianceM2 = 999.0f,
                opticalFlowScore = 0.0f,
                lateralBias = 0.0f,
            )
        }

        val output = nativeEskfUpdateVisualOdometryRgba(
            handle = handle,
            rgbaBytes = rgbaBytes,
            width = width,
            height = height,
            dtS = dtS,
        )
        return VisionOdometryResult(
            applied = output.getOrElse(0) { 0.0f } > 0.5f,
            deltaXM = output.getOrElse(1) { 0.0f },
            deltaYM = output.getOrElse(2) { 0.0f },
            poseXM = output.getOrElse(3) { 0.0f },
            poseYM = output.getOrElse(4) { 0.0f },
            varianceM2 = output.getOrElse(5) { 999.0f },
            opticalFlowScore = output.getOrElse(6) { 0.0f },
            lateralBias = output.getOrElse(7) { 0.0f },
        )
    }

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
