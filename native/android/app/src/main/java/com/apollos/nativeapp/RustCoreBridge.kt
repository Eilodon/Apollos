package com.apollos.nativeapp

import java.nio.ByteBuffer

data class KinematicResult(
    val riskScore: Float,
    val shouldCapture: Boolean,
    val yawDeltaDeg: Float,
)

data class DepthHazardResult(
    val detected: Boolean,
    val positionX: Float,
    val confidence: Float,
    val distanceCode: Int,
    val distanceM: Float,
    val relativeVelocityMps: Float,
    val timeToCollisionS: Float?,
)

data class EdgeObjectDetection(
    val labelId: Int,
    val xMin: Float,
    val yMin: Float,
    val xMax: Float,
    val yMax: Float,
    val confidence: Float,
    val medianDepthM: Float,
    val minDepthM: Float,
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

    private external fun nativeComputeYawDelta(
        alphaDegPerSecond: Float,
        dtMs: Float,
    ): Float

    private external fun nativeDepthOnnxEnabled(): Int


    private external fun nativeDetectDropAheadObjects(
        objectVector: FloatArray,
        objectCount: Int,
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


    fun analyzeKinematics(
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
        sensorUnavailable: Boolean,
    ): KinematicResult {
        val output = nativeAnalyzeKinematics(
            motionStateCode,
            carryModeCode,
            pitch,
            velocity,
            yawDeltaDeg,
            accelX,
            accelY,
            accelZ,
            gyroAlpha,
            gyroBeta,
            gyroGamma,
            if (sensorUnavailable) 1 else 0,
        )

        return KinematicResult(
            riskScore = output.getOrElse(0) { 0.0f },
            shouldCapture = output.getOrElse(1) { 0.0f } > 0.5f,
            yawDeltaDeg = output.getOrElse(2) { 0.0f },
        )
    }

    fun computeYawDelta(alphaDegPerSecond: Float, dtMs: Float): Float {
        return nativeComputeYawDelta(alphaDegPerSecond, dtMs)
    }


    fun detectDropAheadObjects(
        objects: List<EdgeObjectDetection>,
        riskScore: Float,
        carryModeCode: Byte,
        gyroMagnitude: Float,
        nowMs: Long,
    ): DepthHazardResult {
        if (objects.isEmpty()) {
            return DepthHazardResult(
                detected = false,
                positionX = 0.0f,
                confidence = 0.0f,
                distanceCode = 0,
                distanceM = 0.0f,
                relativeVelocityMps = 0.0f,
                timeToCollisionS = null,
            )
        }

        val count = objects.size.coerceAtMost(32)
        val packed = FloatArray(count * 8)
        for (idx in 0 until count) {
            val obj = objects[idx]
            val base = idx * 8
            packed[base] = obj.labelId.toFloat().coerceAtLeast(0.0f)
            packed[base + 1] = obj.xMin.coerceIn(0.0f, 1.0f)
            packed[base + 2] = obj.yMin.coerceIn(0.0f, 1.0f)
            packed[base + 3] = obj.xMax.coerceIn(0.0f, 1.0f)
            packed[base + 4] = obj.yMax.coerceIn(0.0f, 1.0f)
            packed[base + 5] = obj.confidence.coerceIn(0.0f, 1.0f)
            packed[base + 6] = obj.medianDepthM.coerceAtLeast(0.0f)
            packed[base + 7] = obj.minDepthM.coerceAtLeast(0.0f)
        }

        val output = nativeDetectDropAheadObjects(
            objectVector = packed,
            objectCount = count,
            riskScore = riskScore,
            carryModeCode = carryModeCode,
            gyroMagnitude = gyroMagnitude,
            nowMs = nowMs,
        )
        return DepthHazardResult(
            detected = output.getOrElse(0) { 0.0f } > 0.5f,
            positionX = output.getOrElse(1) { 0.0f },
            confidence = output.getOrElse(2) { 0.0f },
            distanceCode = output.getOrElse(3) { 0.0f }.toInt(),
            distanceM = output.getOrElse(4) { 0.0f }.coerceAtLeast(0.0f),
            relativeVelocityMps = output.getOrElse(5) { 0.0f },
            timeToCollisionS = output.getOrNull(6)?.takeIf { it.isFinite() && it > 0.0f },
        )
    }
}
