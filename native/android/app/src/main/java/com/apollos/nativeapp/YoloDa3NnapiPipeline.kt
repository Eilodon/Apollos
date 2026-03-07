package com.apollos.nativeapp

import android.content.Context
import androidx.camera.core.ImageProxy
import org.tensorflow.lite.DataType
import org.tensorflow.lite.Interpreter
import org.tensorflow.lite.gpu.GpuDelegate
import org.tensorflow.lite.nnapi.NnApiDelegate
import java.io.FileInputStream
import java.nio.ByteBuffer
import java.nio.ByteOrder
import java.nio.MappedByteBuffer
import java.nio.channels.FileChannel
import kotlin.math.max
import kotlin.math.min

internal data class EdgeInferenceResult(
    val objects: List<EdgeObjectDetection>,
    val feedAvailable: Boolean,
)

internal class YoloDa3NnapiPipeline(
    private val context: Context,
    private val logger: (String) -> Unit = {},
) {
    companion object {
        private val YOLO_MODEL_CANDIDATES = listOf(
            "models/yolov12.tflite",
            "models/yolo_v12.tflite",
            "models/yolo.tflite",
        )
        private val DEPTH_MODEL_CANDIDATES = listOf(
            "models/depth_anything_v3.tflite",
            "models/da3.tflite",
            "models/depth.tflite",
        )
        private const val CONFIDENCE_THRESHOLD = 0.35f
        private const val NMS_IOU_THRESHOLD = 0.45f
        private const val MAX_OBJECTS = 16
    }

    private data class ModelRuntime(
        val interpreter: Interpreter,
        val delegate: AutoCloseable?,
        val inputWidth: Int,
        val inputHeight: Int,
        val inputChannels: Int,
    )

    private data class RawDetection(
        val labelId: Int,
        val confidence: Float,
        val xMin: Float,
        val yMin: Float,
        val xMax: Float,
        val yMax: Float,
    )

    private data class DepthStats(
        val low: Float,
        val high: Float,
        val metricLike: Boolean,
    )

    private data class DepthMap(
        val width: Int,
        val height: Int,
        val values: FloatArray,
        val stats: DepthStats,
    )

    @Volatile
    private var available = false

    @Volatile
    private var unavailableReason: String? = null

    private var yoloRuntime: ModelRuntime? = null
    private var depthRuntime: ModelRuntime? = null

    init {
        initialize()
    }

    fun isAvailable(): Boolean = available

    fun unavailableReason(): String? = unavailableReason

    fun close() {
        yoloRuntime?.interpreter?.close()
        yoloRuntime?.delegate?.close()
        yoloRuntime = null

        depthRuntime?.interpreter?.close()
        depthRuntime?.delegate?.close()
        depthRuntime = null

        available = false
    }

    fun detect(image: ImageProxy): EdgeInferenceResult {
        val rotationDegrees = image.imageInfo.rotationDegrees
        val yolo = yoloRuntime
        val depth = depthRuntime
        if (yolo == null || depth == null) {
            return EdgeInferenceResult(objects = emptyList(), feedAvailable = false)
        }

        val rgba = image.planes.firstOrNull() ?: return EdgeInferenceResult(
            objects = emptyList(),
            feedAvailable = false,
        )
        if (rgba.pixelStride < 4) {
            return EdgeInferenceResult(objects = emptyList(), feedAvailable = false)
        }

        val yoloInput = preprocessRgba(
            rgbaBuffer = rgba.buffer,
            srcWidth = image.width,
            srcHeight = image.height,
            srcRowStride = rgba.rowStride,
            srcPixelStride = rgba.pixelStride,
            dstWidth = yolo.inputWidth,
            dstHeight = yolo.inputHeight,
            dstChannels = yolo.inputChannels,
            rotationDegrees = rotationDegrees,
        )
        val yoloOutput = ByteBuffer.allocateDirect(yolo.interpreter.getOutputTensor(0).numBytes())
            .order(ByteOrder.nativeOrder())
        runCatching {
            yolo.interpreter.run(yoloInput, yoloOutput)
        }.onFailure {
            return EdgeInferenceResult(objects = emptyList(), feedAvailable = false)
        }

        val detections = decodeYoloOutput(
            interpreter = yolo.interpreter,
            output = yoloOutput,
            inputWidth = yolo.inputWidth,
            inputHeight = yolo.inputHeight,
        )
        if (detections.isEmpty()) {
            return EdgeInferenceResult(objects = emptyList(), feedAvailable = true)
        }

        val depthInput = preprocessRgba(
            rgbaBuffer = rgba.buffer,
            srcWidth = image.width,
            srcHeight = image.height,
            srcRowStride = rgba.rowStride,
            srcPixelStride = rgba.pixelStride,
            dstWidth = depth.inputWidth,
            dstHeight = depth.inputHeight,
            dstChannels = depth.inputChannels,
            rotationDegrees = rotationDegrees,
        )
        val depthOutput = ByteBuffer.allocateDirect(depth.interpreter.getOutputTensor(0).numBytes())
            .order(ByteOrder.nativeOrder())
        runCatching {
            depth.interpreter.run(depthInput, depthOutput)
        }.onFailure {
            return EdgeInferenceResult(objects = emptyList(), feedAvailable = false)
        }

        val depthMap = decodeDepthMap(depth.interpreter, depthOutput)
            ?: return EdgeInferenceResult(objects = emptyList(), feedAvailable = false)
        val fused = detections.mapNotNull { det ->
            val spatials = sampleDepthForBox(depthMap, det)
            EdgeObjectDetection(
                labelId = det.labelId,
                xMin = det.xMin,
                yMin = det.yMin,
                xMax = det.xMax,
                yMax = det.yMax,
                confidence = det.confidence,
                medianDepthM = spatials.first,
                minDepthM = spatials.second,
            )
        }

        return EdgeInferenceResult(objects = fused, feedAvailable = true)
    }

    private fun initialize() {
        val yoloPath = firstExistingAsset(YOLO_MODEL_CANDIDATES)
        val depthPath = firstExistingAsset(DEPTH_MODEL_CANDIDATES)
        if (yoloPath == null || depthPath == null) {
            available = false
            unavailableReason = "missing model assets: yolo=$yoloPath depth=$depthPath"
            logger("Edge model runtime unavailable: ${unavailableReason ?: "unknown"}")
            return
        }

        val yolo = runCatching { createRuntime(yoloPath) }.getOrElse { error ->
            available = false
            unavailableReason = "YOLO init failed: ${error.message}"
            logger("Edge model runtime unavailable: ${unavailableReason ?: "unknown"}")
            return
        }
        val depth = runCatching { createRuntime(depthPath) }.getOrElse { error ->
            yolo.interpreter.close()
            yolo.delegate?.close()
            available = false
            unavailableReason = "Depth init failed: ${error.message}"
            logger("Edge model runtime unavailable: ${unavailableReason ?: "unknown"}")
            return
        }

        yoloRuntime = yolo
        depthRuntime = depth
        available = true
        unavailableReason = null
        logger(
            "Edge model runtime ready (NNAPI/XNNPACK): yolo=${yolo.inputWidth}x${yolo.inputHeight}, " +
                "depth=${depth.inputWidth}x${depth.inputHeight}",
        )
    }

    private fun firstExistingAsset(candidates: List<String>): String? {
        for (path in candidates) {
            val ok = runCatching {
                context.assets.openFd(path).close()
                true
            }.getOrDefault(false)
            if (ok) {
                return path
            }
        }
        return null
    }

    private fun createRuntime(assetPath: String): ModelRuntime {
        val model = loadModelFile(assetPath)
        val options = Interpreter.Options()
            .setNumThreads(4)
            .setUseXNNPACK(true)
        
        var delegate: AutoCloseable? = null
        
        // Strategy: 1. GPU (Most consistent) -> 2. NNAPI (OEM Optimized) -> 3. CPU (XNNPACK)
        val gpu = runCatching { GpuDelegate() }.getOrNull()
        if (gpu != null) {
            options.addDelegate(gpu)
            delegate = gpu
            logger("Using GPU acceleration for $assetPath")
        } else {
            val nnapi = runCatching { NnApiDelegate() }.getOrNull()
            if (nnapi != null) {
                options.addDelegate(nnapi)
                delegate = nnapi
                logger("Using NNAPI acceleration for $assetPath")
            } else {
                logger("No acceleration hardware found for $assetPath, falling back to XNNPACK CPU")
            }
        }
        val interpreter = Interpreter(model, options)
        val input = interpreter.getInputTensor(0)
        val shape = input.shape()
        require(shape.size == 4) { "unsupported input rank ${shape.size} for $assetPath" }
        require(input.dataType() == DataType.FLOAT32) {
            "only float32 input is supported for $assetPath"
        }

        val channelLast = shape[3] in 1..4
        val inputHeight: Int
        val inputWidth: Int
        val inputChannels: Int
        if (channelLast) {
            inputHeight = shape[1]
            inputWidth = shape[2]
            inputChannels = shape[3]
        } else {
            inputChannels = shape[1]
            inputHeight = shape[2]
            inputWidth = shape[3]
        }
        require(inputWidth > 0 && inputHeight > 0 && inputChannels in 1..4) {
            "invalid input shape for $assetPath: ${shape.joinToString("x")}"
        }

        return ModelRuntime(
            interpreter = interpreter,
            delegate = delegate,
            inputWidth = inputWidth,
            inputHeight = inputHeight,
            inputChannels = inputChannels,
        )
    }

    private fun loadModelFile(assetPath: String): MappedByteBuffer {
        context.assets.openFd(assetPath).use { fd ->
            FileInputStream(fd.fileDescriptor).channel.use { channel ->
                return channel.map(
                    FileChannel.MapMode.READ_ONLY,
                    fd.startOffset,
                    fd.declaredLength,
                )
            }
        }
    }

    private fun preprocessRgba(
        rgbaBuffer: ByteBuffer,
        srcWidth: Int,
        srcHeight: Int,
        srcRowStride: Int,
        srcPixelStride: Int,
        dstWidth: Int,
        dstHeight: Int,
        dstChannels: Int,
        rotationDegrees: Int,
    ): ByteBuffer {
        val output = ByteBuffer.allocateDirect(dstWidth * dstHeight * dstChannels * 4)
            .order(ByteOrder.nativeOrder())
        val src = rgbaBuffer.duplicate()
        
        // Final upright dimensions after rotation
        val is90or270 = rotationDegrees == 90 || rotationDegrees == 270
        val finalSrcWidth = if (is90or270) srcHeight else srcWidth
        val finalSrcHeight = if (is90or270) srcWidth else srcHeight

        for (dy in 0 until dstHeight) {
            val fy = dy.toFloat() / dstHeight.toFloat()
            for (dx in 0 until dstWidth) {
                val fx = dx.toFloat() / dstWidth.toFloat()
                
                // Map upright normalized coord (fx, fy) back to raw sensor coord (sx, sy)
                val (sx, sy) = when (rotationDegrees) {
                    90 -> {
                        // (fx, fy) in upright -> x increases along reverse sensor-Y, y increases along sensor-X
                        val sX = (fy * (srcWidth - 1)).toInt()
                        val sY = ((1.0f - fx) * (srcHeight - 1)).toInt()
                        sX to sY
                    }
                    180 -> {
                        val sX = ((1.0f - fx) * (srcWidth - 1)).toInt()
                        val sY = ((1.0f - fy) * (srcHeight - 1)).toInt()
                        sX to sY
                    }
                    270 -> {
                        val sX = ((1.0f - fy) * (srcWidth - 1)).toInt()
                        val sY = (fx * (srcHeight - 1)).toInt()
                        sX to sY
                    }
                    else -> { // 0 or unknown
                        val sX = (fx * (srcWidth - 1)).toInt()
                        val sY = (fy * (srcHeight - 1)).toInt()
                        sX to sY
                    }
                }

                val index = sy * srcRowStride + sx * srcPixelStride
                val r = (src.get(index).toInt() and 0xFF) / 255.0f
                val g = (src.get(index + 1).toInt() and 0xFF) / 255.0f
                val b = (src.get(index + 2).toInt() and 0xFF) / 255.0f
                when (dstChannels) {
                    1 -> output.putFloat((r + g + b) / 3.0f)
                    3 -> {
                        output.putFloat(r)
                        output.putFloat(g)
                        output.putFloat(b)
                    }
                    4 -> {
                        val a = (src.get(index + 3).toInt() and 0xFF) / 255.0f
                        output.putFloat(r)
                        output.putFloat(g)
                        output.putFloat(b)
                        output.putFloat(a)
                    }
                }
            }
        }
        output.rewind()
        return output
    }

    private fun decodeYoloOutput(
        interpreter: Interpreter,
        output: ByteBuffer,
        inputWidth: Int,
        inputHeight: Int,
    ): List<RawDetection> {
        val tensor = interpreter.getOutputTensor(0)
        if (tensor.dataType() != DataType.FLOAT32) {
            return emptyList()
        }
        val shape = tensor.shape()
        val data = FloatArray(output.asFloatBuffer().remaining())
        output.asFloatBuffer().get(data)

        val count: Int
        val features: Int
        val valueAt: (Int, Int) -> Float
        when (shape.size) {
            3 -> {
                val a = shape[1]
                val b = shape[2]
                if (a in 5..512 && b > a) {
                    features = a
                    count = b
                    valueAt = { c, f -> data[f * count + c] }
                } else {
                    count = a
                    features = b
                    valueAt = { c, f -> data[c * features + f] }
                }
            }
            2 -> {
                count = shape[0]
                features = shape[1]
                valueAt = { c, f -> data[c * features + f] }
            }
            else -> return emptyList()
        }
        if (features < 5 || count <= 0) {
            return emptyList()
        }

        val candidates = ArrayList<RawDetection>(count)
        for (c in 0 until count) {
            val cx = valueAt(c, 0)
            val cy = valueAt(c, 1)
            val w = valueAt(c, 2)
            val h = valueAt(c, 3)
            if (!cx.isFinite() || !cy.isFinite() || !w.isFinite() || !h.isFinite() || w <= 0f || h <= 0f) {
                continue
            }

            val objectness = valueAt(c, 4).coerceIn(0.0f, 1.0f)
            var bestClass = 0
            var bestClassScore = 1.0f
            if (features > 5) {
                bestClassScore = 0.0f
                for (idx in 5 until features) {
                    val score = valueAt(c, idx)
                    if (score > bestClassScore) {
                        bestClassScore = score
                        bestClass = idx - 5
                    }
                }
                bestClassScore = bestClassScore.coerceIn(0.0f, 1.0f)
            }
            val confidence = (objectness * bestClassScore).coerceIn(0.0f, 1.0f)
            if (confidence < CONFIDENCE_THRESHOLD) {
                continue
            }

            val xCenter = if (cx > 1.0f) cx / inputWidth.toFloat() else cx
            val yCenter = if (cy > 1.0f) cy / inputHeight.toFloat() else cy
            val boxW = if (w > 1.0f) w / inputWidth.toFloat() else w
            val boxH = if (h > 1.0f) h / inputHeight.toFloat() else h

            val xMin = (xCenter - boxW * 0.5f).coerceIn(0.0f, 1.0f)
            val yMin = (yCenter - boxH * 0.5f).coerceIn(0.0f, 1.0f)
            val xMax = (xCenter + boxW * 0.5f).coerceIn(0.0f, 1.0f)
            val yMax = (yCenter + boxH * 0.5f).coerceIn(0.0f, 1.0f)
            if (xMax <= xMin || yMax <= yMin) {
                continue
            }

            candidates.add(
                RawDetection(
                    labelId = bestClass,
                    confidence = confidence,
                    xMin = xMin,
                    yMin = yMin,
                    xMax = xMax,
                    yMax = yMax,
                ),
            )
        }

        return applyNms(candidates)
    }

    private fun applyNms(input: List<RawDetection>): List<RawDetection> {
        if (input.isEmpty()) {
            return emptyList()
        }
        val sorted = input.sortedByDescending { it.confidence }
        val selected = ArrayList<RawDetection>(min(MAX_OBJECTS, sorted.size))
        for (candidate in sorted) {
            if (selected.size >= MAX_OBJECTS) {
                break
            }
            var overlaps = false
            for (existing in selected) {
                if (iou(candidate, existing) > NMS_IOU_THRESHOLD) {
                    overlaps = true
                    break
                }
            }
            if (!overlaps) {
                selected.add(candidate)
            }
        }
        return selected
    }

    private fun iou(a: RawDetection, b: RawDetection): Float {
        val xA = max(a.xMin, b.xMin)
        val yA = max(a.yMin, b.yMin)
        val xB = min(a.xMax, b.xMax)
        val yB = min(a.yMax, b.yMax)
        val interW = (xB - xA).coerceAtLeast(0.0f)
        val interH = (yB - yA).coerceAtLeast(0.0f)
        val intersection = interW * interH
        if (intersection <= 0.0f) {
            return 0.0f
        }
        val areaA = (a.xMax - a.xMin) * (a.yMax - a.yMin)
        val areaB = (b.xMax - b.xMin) * (b.yMax - b.yMin)
        val union = areaA + areaB - intersection
        if (union <= 1e-6f) {
            return 0.0f
        }
        return (intersection / union).coerceIn(0.0f, 1.0f)
    }

    private fun decodeDepthMap(interpreter: Interpreter, output: ByteBuffer): DepthMap? {
        val tensor = interpreter.getOutputTensor(0)
        if (tensor.dataType() != DataType.FLOAT32) {
            return null
        }
        val shape = tensor.shape()
        val flat = FloatArray(output.asFloatBuffer().remaining())
        output.asFloatBuffer().get(flat)

        val width: Int
        val height: Int
        val values: FloatArray
        when (shape.size) {
            4 -> {
                if (shape[1] == 1) {
                    height = shape[2]
                    width = shape[3]
                    values = FloatArray(width * height)
                    var idx = 0
                    for (y in 0 until height) {
                        for (x in 0 until width) {
                            values[idx++] = flat[(y * width) + x]
                        }
                    }
                } else if (shape[3] == 1) {
                    height = shape[1]
                    width = shape[2]
                    values = FloatArray(width * height)
                    var idx = 0
                    for (y in 0 until height) {
                        for (x in 0 until width) {
                            values[idx++] = flat[(y * width + x)]
                        }
                    }
                } else {
                    return null
                }
            }
            3 -> {
                height = shape[1]
                width = shape[2]
                values = FloatArray(width * height)
                var idx = 0
                for (y in 0 until height) {
                    for (x in 0 until width) {
                        values[idx++] = flat[(y * width) + x]
                    }
                }
            }
            2 -> {
                height = shape[0]
                width = shape[1]
                values = flat.copyOf(width * height)
            }
            else -> return null
        }
        if (width <= 0 || height <= 0 || values.isEmpty()) {
            return null
        }

        val stats = computeDepthStats(values) ?: return null
        return DepthMap(width = width, height = height, values = values, stats = stats)
    }

    private fun computeDepthStats(values: FloatArray): DepthStats? {
        val sample = ArrayList<Float>(512)
        val step = (values.size / 512).coerceAtLeast(1)
        var idx = 0
        while (idx < values.size) {
            val value = values[idx]
            if (value.isFinite() && value > 0.0f) {
                sample.add(value)
            }
            idx += step
        }
        if (sample.size < 8) {
            return null
        }
        sample.sort()
        val p10 = sample[(sample.size * 0.1f).toInt().coerceIn(0, sample.lastIndex)]
        val p90 = sample[(sample.size * 0.9f).toInt().coerceIn(0, sample.lastIndex)]
        val low = min(p10, p90)
        val high = max(p10, p90)
        if (!low.isFinite() || !high.isFinite() || (high - low) < 1e-6f) {
            return null
        }
        val metricLike = low >= 0.05f && high <= 12.0f
        return DepthStats(low = low, high = high, metricLike = metricLike)
    }

    private fun sampleDepthForBox(depthMap: DepthMap, det: RawDetection): Pair<Float, Float> {
        val minX = (det.xMin * (depthMap.width - 1)).toInt().coerceIn(0, depthMap.width - 1)
        val maxX = (det.xMax * (depthMap.width - 1)).toInt().coerceIn(0, depthMap.width - 1)
        val minY = (det.yMin * (depthMap.height - 1)).toInt().coerceIn(0, depthMap.height - 1)
        val maxY = (det.yMax * (depthMap.height - 1)).toInt().coerceIn(0, depthMap.height - 1)
        if (maxX <= minX || maxY <= minY) {
            return 6.0f to 6.0f
        }

        val sx = ((maxX - minX) / 4).coerceAtLeast(1)
        val sy = ((maxY - minY) / 4).coerceAtLeast(1)
        val samples = ArrayList<Float>(32)
        var y = minY
        while (y <= maxY) {
            var x = minX
            while (x <= maxX) {
                val raw = depthMap.values[y * depthMap.width + x]
                val meters = rawDepthToMeters(raw, depthMap.stats)
                if (meters.isFinite() && meters > 0.0f) {
                    samples.add(meters)
                }
                x += sx
            }
            y += sy
        }
        if (samples.isEmpty()) {
            return 6.0f to 6.0f
        }

        samples.sort()
        val median = samples[samples.size / 2].coerceIn(0.25f, 8.0f)
        val minDepth = samples.first().coerceIn(0.25f, 8.0f)
        return median to minDepth
    }

    private fun rawDepthToMeters(raw: Float, stats: DepthStats): Float {
        if (!raw.isFinite() || raw <= 0.0f) {
            return 6.0f
        }
        if (stats.metricLike) {
            return raw.coerceIn(0.25f, 8.0f)
        }
        val span = (stats.high - stats.low).coerceAtLeast(1e-6f)
        val normalized = ((raw - stats.low) / span).coerceIn(0.0f, 1.0f)
        val proximity = normalized // Depth Anything often exports inverse depth (larger => closer).
        val nearM = 0.35f
        val farM = 6.0f
        return (nearM + (1.0f - proximity) * (farM - nearM)).coerceIn(nearM, farM)
    }
}
