package com.apollos.nativeapp

import android.Manifest
import android.content.Context
import android.content.pm.PackageManager
import android.media.AudioAttributes
import android.media.AudioFormat
import android.media.AudioRecord
import android.media.AudioTrack
import android.media.MediaRecorder
import android.os.Build
import android.os.SystemClock
import android.os.VibrationEffect
import android.os.Vibrator
import android.os.VibratorManager
import android.hardware.Sensor
import android.hardware.SensorEvent
import android.hardware.SensorEventListener
import android.hardware.SensorManager
import android.util.Base64
import androidx.camera.core.CameraSelector
import androidx.camera.core.ImageAnalysis
import androidx.camera.lifecycle.ProcessCameraProvider
import androidx.core.content.ContextCompat
import androidx.lifecycle.LifecycleOwner
import com.google.android.gms.location.LocationCallback
import com.google.android.gms.location.LocationRequest
import com.google.android.gms.location.LocationResult
import com.google.android.gms.location.LocationServices
import com.google.android.gms.location.Priority
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancelAndJoin
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import okhttp3.MediaType.Companion.toMediaType
import okhttp3.OkHttpClient
import okhttp3.Request
import okhttp3.RequestBody.Companion.toRequestBody
import okhttp3.Response
import okhttp3.WebSocket
import okhttp3.WebSocketListener
import org.json.JSONObject
import java.time.Instant
import java.util.UUID
import java.util.concurrent.Executors
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicLong
import kotlin.math.cos
import kotlin.math.sin
import kotlin.math.sqrt

data class LocationSnapshot(
    val lat: Double,
    val lng: Double,
    val accuracyM: Float,
)

class RealtimeSessionManager(
    private val context: Context,
    private val lifecycleOwner: LifecycleOwner,
) {
    companion object {
        private const val SAFETY_DEADZONE_SCORE = 0.1f
        private const val SAFETY_TONE_SAMPLE_RATE = 16_000
    }

    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Main.immediate)
    private val network = OkHttpClient.Builder()
        .connectTimeout(12, TimeUnit.SECONDS)
        .readTimeout(0, TimeUnit.MILLISECONDS)
        .build()
    private val cameraExecutor = Executors.newSingleThreadExecutor()

    private val locationClient = LocationServices.getFusedLocationProviderClient(context)
    private val sensorManager = context.getSystemService(Context.SENSOR_SERVICE) as? SensorManager
    private var locationCallback: LocationCallback? = null
    private var sensorListener: SensorEventListener? = null
    private var latestLocation: LocationSnapshot? = null
    private var geoOrigin: LocationSnapshot? = null
    @Volatile private var eskfHandle: Long = 0L
    @Volatile private var lastImuTimestampNs: Long = 0L
    @Volatile private var lastCameraFrameTimestampNs: Long = 0L
    @Volatile private var latestGyroMagnitudeDeg: Float = 0.0f
    @Volatile private var latestVoApplied: Boolean = false
    @Volatile private var latestVoFlowScore: Float = 0.0f
    @Volatile private var latestVoVarianceM2: Float = 999.0f
    @Volatile private var latestVoPoseXM: Float = 0.0f
    @Volatile private var latestVoPoseYM: Float = 0.0f
    private var gravityEstimate = FloatArray(3)

    private var sessionId: String = UUID.randomUUID().toString()
    private var webSocket: WebSocket? = null
    private var audioJob: Job? = null
    private var running = false
    private var wsOpen = false
    private val lastFrameSentAtMs = AtomicLong(0L)
    private val safetyActuatorLock = Any()
    private var safetyToneTrack: AudioTrack? = null
    private var latestSafetyDirectiveAtMs: Long = 0L

    suspend fun start(
        serverBaseUrl: String,
        idToken: String,
        onStatus: (String) -> Unit,
    ): Boolean {
        if (running) return true
        sessionId = UUID.randomUUID().toString()
        wsOpen = false

        val (_, wsToken) = try {
            fetchAuthTokens(serverBaseUrl, idToken)
        } catch (error: Throwable) {
            onStatus("Auth failed: ${error.message}")
            return false
        }
        running = true
        onStatus("Auth OK")
        eskfHandle = RustCoreBridge.eskfCreate()
        if (eskfHandle == 0L) {
            running = false
            onStatus("ESKF create failed")
            return false
        }
        geoOrigin = null
        lastImuTimestampNs = 0L
        lastCameraFrameTimestampNs = 0L
        latestGyroMagnitudeDeg = 0.0f
        latestVoApplied = false
        latestVoFlowScore = 0.0f
        latestVoVarianceM2 = 999.0f
        latestVoPoseXM = 0.0f
        latestVoPoseYM = 0.0f
        gravityEstimate = FloatArray(3)
        latestSafetyDirectiveAtMs = 0L

        connectLiveSocket(serverBaseUrl, wsToken, onStatus)
        startLocation(onStatus)
        startSensors(onStatus)
        startAudio(onStatus)
        startCamera(onStatus)

        onStatus("Live session started: $sessionId")
        return true
    }

    suspend fun stop(onStatus: (String) -> Unit) {
        if (!running) {
            if (eskfHandle != 0L) {
                RustCoreBridge.eskfDestroy(eskfHandle)
                eskfHandle = 0L
            }
            return
        }
        running = false

        webSocket?.close(1000, "client_stop")
        webSocket = null
        wsOpen = false

        locationCallback?.let { callback ->
            locationClient.removeLocationUpdates(callback)
        }
        locationCallback = null
        latestLocation = null
        geoOrigin = null

        sensorListener?.let { listener ->
            sensorManager?.unregisterListener(listener)
        }
        sensorListener = null
        lastImuTimestampNs = 0L
        lastCameraFrameTimestampNs = 0L
        latestGyroMagnitudeDeg = 0.0f
        latestVoApplied = false
        latestVoFlowScore = 0.0f
        latestVoVarianceM2 = 999.0f
        latestVoPoseXM = 0.0f
        latestVoPoseYM = 0.0f
        latestSafetyDirectiveAtMs = 0L
        if (eskfHandle != 0L) {
            RustCoreBridge.eskfDestroy(eskfHandle)
            eskfHandle = 0L
        }

        audioJob?.cancelAndJoin()
        audioJob = null

        runCatching {
            ProcessCameraProvider.getInstance(context).get().unbindAll()
        }.onFailure { error ->
            onStatus("Camera cleanup failed: ${error.message}")
        }
        stopSafetyActuator()
        onStatus("Stopped")
    }

    private suspend fun fetchAuthTokens(
        serverBaseUrl: String,
        idToken: String,
    ): Pair<String, String> = withContext(Dispatchers.IO) {
        val exchange = JSONObject()
            .put("id_token", idToken.trim())
            .toString()
        val exchangeBody = postJson("$serverBaseUrl/auth/oidc/exchange", exchange)
        val sessionToken = JSONObject(exchangeBody).getString("session_token")

        val wsReq = JSONObject().put("session_token", sessionToken).toString()
        val wsBody = postJson("$serverBaseUrl/auth/ws-ticket", wsReq)
        val wsToken = JSONObject(wsBody).getString("access_token")
        sessionToken to wsToken
    }

    private fun connectLiveSocket(serverBaseUrl: String, wsToken: String, onStatus: (String) -> Unit) {
        val wsBase = serverBaseUrl
            .replaceFirst("https://", "wss://")
            .replaceFirst("http://", "ws://")
            .trimEnd('/')
        val tokenB64 = Base64.encodeToString(
            wsToken.toByteArray(Charsets.UTF_8),
            Base64.URL_SAFE or Base64.NO_WRAP or Base64.NO_PADDING,
        )
        val protocolHeader = "authb64.$tokenB64, apollos.v1"

        val request = Request.Builder()
            .url("$wsBase/ws/live/$sessionId")
            .header("Sec-WebSocket-Protocol", protocolHeader)
            .build()

        webSocket = network.newWebSocket(request, object : WebSocketListener() {
            override fun onOpen(webSocket: WebSocket, response: Response) {
                wsOpen = true
                onStatus("WS connected")
            }

            override fun onMessage(webSocket: WebSocket, text: String) {
                handleServerMessage(text, onStatus)
            }

            override fun onClosing(webSocket: WebSocket, code: Int, reason: String) {
                onStatus("WS closing: $code $reason")
            }

            override fun onClosed(webSocket: WebSocket, code: Int, reason: String) {
                wsOpen = false
                onStatus("WS closed: $code")
            }

            override fun onFailure(webSocket: WebSocket, t: Throwable, response: Response?) {
                wsOpen = false
                onStatus("WS failure: ${t.message}")
            }
        })
    }

    private fun handleServerMessage(text: String, onStatus: (String) -> Unit) {
        val payload = runCatching { JSONObject(text) }.getOrNull()
        if (payload == null) {
            onStatus("WS non-json message: ${text.take(80)}")
            return
        }

        when (payload.optString("type")) {
            "safety_directive" -> applySafetyDirective(payload, onStatus)
            "connection_state" -> {
                val detail = payload.optString("detail").ifBlank { "state_update" }
                onStatus("WS state: $detail")
            }
            "cognition_state" -> {
                val layer = payload.optString("active_layer").ifBlank { "unknown_layer" }
                val reason = payload.optString("reason").ifBlank { "no_reason" }
                onStatus("WS cognition: $layer ($reason)")
            }
            else -> onStatus("WS message: ${text.take(80)}")
        }
    }

    private fun applySafetyDirective(payload: JSONObject, onStatus: (String) -> Unit) {
        val hazardScore = payload.optDouble("hazard_score", 0.0).toFloat().coerceAtLeast(0.0f)
        val hardStop = payload.optBoolean("hard_stop", false)
        val hapticIntensity = payload.optDouble("haptic_intensity", 0.0).toFloat().coerceIn(0.0f, 1.0f)
        val pitchHz = payload.optDouble("spatial_audio_pitch_hz", 0.0).toFloat().coerceAtLeast(0.0f)
        val nowMs = SystemClock.elapsedRealtime()
        if ((nowMs - latestSafetyDirectiveAtMs) < 60) {
            return
        }
        latestSafetyDirectiveAtMs = nowMs

        if (hazardScore < SAFETY_DEADZONE_SCORE || pitchHz <= 0.0f || hapticIntensity <= 0.0f) {
            stopSafetyActuator()
            return
        }

        val durationMs = if (hardStop) 220 else 90
        val gain = hapticIntensity.coerceIn(0.08f, 1.0f)
        playSafetyTone(pitchHz, gain, durationMs, onStatus)
        triggerSafetyVibration(hapticIntensity, durationMs)
    }

    private fun playSafetyTone(
        frequencyHz: Float,
        gain: Float,
        durationMs: Int,
        onStatus: (String) -> Unit,
    ) {
        scope.launch(Dispatchers.Default) {
            val sampleCount = (SAFETY_TONE_SAMPLE_RATE * durationMs / 1000).coerceAtLeast(1)
            val samples = ShortArray(sampleCount)
            val gainClamped = gain.coerceIn(0.0f, 1.0f)
            val w = (2.0 * Math.PI * frequencyHz.toDouble() / SAFETY_TONE_SAMPLE_RATE.toDouble()).toFloat()
            var phase = 0.0f

            for (idx in 0 until sampleCount) {
                // Fade-out envelope to avoid audible clicks.
                val envelope = 1.0f - (idx.toFloat() / sampleCount.toFloat())
                val value = sin(phase.toDouble()).toFloat() * gainClamped * envelope
                samples[idx] = (value * Short.MAX_VALUE).toInt().coerceIn(
                    Short.MIN_VALUE.toInt(),
                    Short.MAX_VALUE.toInt(),
                ).toShort()
                phase += w
            }

            val track = try {
                AudioTrack.Builder()
                    .setAudioAttributes(
                        AudioAttributes.Builder()
                            .setUsage(AudioAttributes.USAGE_ASSISTANCE_SONIFICATION)
                            .setContentType(AudioAttributes.CONTENT_TYPE_SONIFICATION)
                            .build(),
                    )
                    .setAudioFormat(
                        AudioFormat.Builder()
                            .setSampleRate(SAFETY_TONE_SAMPLE_RATE)
                            .setEncoding(AudioFormat.ENCODING_PCM_16BIT)
                            .setChannelMask(AudioFormat.CHANNEL_OUT_MONO)
                            .build(),
                    )
                    .setTransferMode(AudioTrack.MODE_STATIC)
                    .setBufferSizeInBytes(sampleCount * 2)
                    .build()
            } catch (error: Throwable) {
                onStatus("Safety tone init failed: ${error.message}")
                return@launch
            }

            synchronized(safetyActuatorLock) {
                safetyToneTrack?.release()
                safetyToneTrack = track
            }

            runCatching {
                track.write(samples, 0, samples.size, AudioTrack.WRITE_BLOCKING)
                track.play()
            }.onFailure { error ->
                onStatus("Safety tone play failed: ${error.message}")
                synchronized(safetyActuatorLock) {
                    if (safetyToneTrack === track) {
                        safetyToneTrack = null
                    }
                }
                track.release()
            }
        }
    }

    private fun triggerSafetyVibration(intensity: Float, durationMs: Int) {
        val vibrator = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
            val manager = context.getSystemService(Context.VIBRATOR_MANAGER_SERVICE) as? VibratorManager
            manager?.defaultVibrator
        } else {
            @Suppress("DEPRECATION")
            context.getSystemService(Context.VIBRATOR_SERVICE) as? Vibrator
        } ?: return

        if (!vibrator.hasVibrator()) {
            return
        }

        val amplitude = (intensity.coerceIn(0.0f, 1.0f) * 255.0f).toInt().coerceIn(1, 255)
        val effect = VibrationEffect.createOneShot(durationMs.toLong(), amplitude)
        vibrator.vibrate(effect)
    }

    private fun stopSafetyActuator() {
        synchronized(safetyActuatorLock) {
            safetyToneTrack?.apply {
                runCatching { stop() }
                release()
            }
            safetyToneTrack = null
        }
    }

    private fun startLocation(onStatus: (String) -> Unit) {
        val fine = ContextCompat.checkSelfPermission(
            context,
            Manifest.permission.ACCESS_FINE_LOCATION,
        ) == PackageManager.PERMISSION_GRANTED
        val coarse = ContextCompat.checkSelfPermission(
            context,
            Manifest.permission.ACCESS_COARSE_LOCATION,
        ) == PackageManager.PERMISSION_GRANTED
        if (!fine && !coarse) {
            onStatus("Location permission missing")
            return
        }

        val request = LocationRequest.Builder(Priority.PRIORITY_HIGH_ACCURACY, 2_000)
            .setMinUpdateIntervalMillis(1_000)
            .build()

        val callback = object : LocationCallback() {
            override fun onLocationResult(result: LocationResult) {
                val latest = result.lastLocation ?: return
                val snapshot = LocationSnapshot(
                    lat = latest.latitude,
                    lng = latest.longitude,
                    accuracyM = latest.accuracy,
                )
                latestLocation = snapshot
                ingestLocationToEskf(snapshot)
            }
        }
        locationCallback = callback
        locationClient.requestLocationUpdates(
            request,
            callback,
            context.mainLooper,
        )
    }

    private fun startSensors(onStatus: (String) -> Unit) {
        val manager = sensorManager
        if (manager == null) {
            onStatus("IMU manager unavailable")
            return
        }

        val accelSensor = manager.getDefaultSensor(Sensor.TYPE_LINEAR_ACCELERATION)
            ?: manager.getDefaultSensor(Sensor.TYPE_ACCELEROMETER)
        val gyroSensor = manager.getDefaultSensor(Sensor.TYPE_GYROSCOPE)

        if (accelSensor == null && gyroSensor == null) {
            onStatus("IMU sensors unavailable")
            return
        }

        val listener = object : SensorEventListener {
            override fun onSensorChanged(event: SensorEvent) {
                if (!running) {
                    return
                }

                when (event.sensor.type) {
                    Sensor.TYPE_LINEAR_ACCELERATION, Sensor.TYPE_ACCELEROMETER -> {
                        val timestampNs = event.timestamp
                        if (lastImuTimestampNs == 0L) {
                            lastImuTimestampNs = timestampNs
                            return
                        }

                        val dtS = ((timestampNs - lastImuTimestampNs).coerceAtLeast(0L) / 1_000_000_000f)
                        lastImuTimestampNs = timestampNs
                        if (dtS <= 0.0f || dtS > 0.2f) {
                            return
                        }

                        val accel = if (event.sensor.type == Sensor.TYPE_LINEAR_ACCELERATION) {
                            floatArrayOf(event.values[0], event.values[1], event.values[2])
                        } else {
                            val alpha = 0.8f
                            for (i in 0..2) {
                                gravityEstimate[i] =
                                    alpha * gravityEstimate[i] + (1.0f - alpha) * event.values[i]
                            }
                            floatArrayOf(
                                event.values[0] - gravityEstimate[0],
                                event.values[1] - gravityEstimate[1],
                                event.values[2] - gravityEstimate[2],
                            )
                        }

                        RustCoreBridge.eskfPredictImu(
                            handle = eskfHandle,
                            accelX = accel[0],
                            accelY = accel[1],
                            accelZ = accel[2],
                            dtS = dtS,
                        )
                    }
                    Sensor.TYPE_GYROSCOPE -> {
                        val x = event.values.getOrElse(0) { 0.0f }
                        val y = event.values.getOrElse(1) { 0.0f }
                        val z = event.values.getOrElse(2) { 0.0f }
                        latestGyroMagnitudeDeg = sqrt(x * x + y * y + z * z) * 57.29578f
                    }
                }
            }

            override fun onAccuracyChanged(sensor: Sensor?, accuracy: Int) = Unit
        }

        sensorListener = listener
        accelSensor?.let {
            manager.registerListener(listener, it, SensorManager.SENSOR_DELAY_GAME)
        }
        gyroSensor?.let {
            manager.registerListener(listener, it, SensorManager.SENSOR_DELAY_GAME)
        }
        onStatus("IMU stream started")
    }

    private fun ingestLocationToEskf(location: LocationSnapshot) {
        val origin = geoOrigin ?: run {
            geoOrigin = location
            location
        }

        val latScaleM = 111_132.0f
        val lngScaleM =
            (111_320.0 * cos(Math.toRadians(origin.lat))).toFloat().coerceAtLeast(1.0f)
        val posX = ((location.lng - origin.lng) * lngScaleM.toDouble()).toFloat()
        val posY = ((location.lat - origin.lat) * latScaleM.toDouble()).toFloat()
        val varianceM2 = location.accuracyM.coerceIn(4.0f, 50.0f).let { it * it }

        RustCoreBridge.eskfUpdateVision(
            handle = eskfHandle,
            positionX = posX,
            positionY = posY,
            positionZ = 0.0f,
            varianceM2 = varianceM2,
        )
    }

    private fun startAudio(onStatus: (String) -> Unit) {
        val granted = ContextCompat.checkSelfPermission(
            context,
            Manifest.permission.RECORD_AUDIO,
        ) == PackageManager.PERMISSION_GRANTED
        if (!granted) {
            onStatus("Mic permission missing")
            return
        }

        audioJob = scope.launch(Dispatchers.IO) {
            val sampleRate = 16_000
            val minBuffer = AudioRecord.getMinBufferSize(
                sampleRate,
                AudioFormat.CHANNEL_IN_MONO,
                AudioFormat.ENCODING_PCM_16BIT,
            ).coerceAtLeast(sampleRate / 2)
            val recorder = AudioRecord(
                MediaRecorder.AudioSource.VOICE_RECOGNITION,
                sampleRate,
                AudioFormat.CHANNEL_IN_MONO,
                AudioFormat.ENCODING_PCM_16BIT,
                minBuffer,
            )
            val buffer = ByteArray(minBuffer)
            try {
                recorder.startRecording()
                while (running) {
                    val read = recorder.read(buffer, 0, buffer.size)
                    if (read <= 0) continue
                    sendAudioChunk(buffer.copyOf(read))
                }
            } finally {
                recorder.stop()
                recorder.release()
            }
        }
    }

    private fun startCamera(onStatus: (String) -> Unit) {
        val granted = ContextCompat.checkSelfPermission(
            context,
            Manifest.permission.CAMERA,
        ) == PackageManager.PERMISSION_GRANTED
        if (!granted) {
            onStatus("Camera permission missing")
            return
        }

        val cameraProviderFuture = ProcessCameraProvider.getInstance(context)
        cameraProviderFuture.addListener(
            {
                val provider = cameraProviderFuture.get()
                val analysis = ImageAnalysis.Builder()
                    .setBackpressureStrategy(ImageAnalysis.STRATEGY_KEEP_ONLY_LATEST)
                    .setOutputImageFormat(ImageAnalysis.OUTPUT_IMAGE_FORMAT_RGBA_8888)
                    .build()
                analysis.setAnalyzer(cameraExecutor) { image ->
                    val plane = image.planes.firstOrNull()
                    if (plane == null) {
                        image.close()
                        return@setAnalyzer
                    }
                    val buffer = plane.buffer
                    buffer.rewind()
                    val rowStride = plane.rowStride
                    val pixelStride = plane.pixelStride
                    val frameTimestampNs = image.imageInfo.timestamp
                    val dtS = if (lastCameraFrameTimestampNs == 0L) {
                        0.033f
                    } else {
                        ((frameTimestampNs - lastCameraFrameTimestampNs).coerceAtLeast(0L) / 1_000_000_000f)
                            .coerceIn(0.01f, 0.2f)
                    }
                    lastCameraFrameTimestampNs = frameTimestampNs

                    val vo = RustCoreBridge.eskfUpdateVisualOdometryRgbaBuffer(
                        handle = eskfHandle,
                        rgbaBuffer = buffer,
                        width = image.width,
                        height = image.height,
                        rowStride = rowStride,
                        pixelStride = pixelStride,
                        dtS = dtS,
                    )
                    latestVoApplied = vo.applied
                    latestVoFlowScore = vo.opticalFlowScore
                    latestVoVarianceM2 = vo.varianceM2
                    latestVoPoseXM = vo.poseXM
                    latestVoPoseYM = vo.poseYM

                    val kinematic = RustCoreBridge.analyzeDefaultWalkingFrame()
                    val depth = RustCoreBridge.detectDropAheadRgbaBuffer(
                        rgbaBuffer = buffer,
                        width = image.width,
                        height = image.height,
                        rowStride = rowStride,
                        pixelStride = pixelStride,
                        riskScore = kinematic.riskScore,
                        carryModeCode = 1,
                        gyroMagnitude = latestGyroMagnitudeDeg,
                        nowMs = SystemClock.elapsedRealtime(),
                    )
                    if (depth.detected) {
                        sendHazardObservation(depth, kinematic.riskScore)
                    }
                    sendMultimodalFrame(kinematic.riskScore, depth)
                    image.close()
                }

                provider.unbindAll()
                provider.bindToLifecycle(
                    lifecycleOwner,
                    CameraSelector.DEFAULT_BACK_CAMERA,
                    analysis,
                )
            },
            ContextCompat.getMainExecutor(context),
        )
    }

    private fun sendMultimodalFrame(riskScore: Float, depth: DepthHazardResult) {
        if (!running || !wsOpen) {
            return
        }
        val nowMs = SystemClock.elapsedRealtime()
        val last = lastFrameSentAtMs.get()
        if (nowMs - last < 200) {
            return
        }
        lastFrameSentAtMs.set(nowMs)

        val location = latestLocation
        val eskf = RustCoreBridge.eskfSnapshot(eskfHandle)
        val flags = mutableListOf<String>()
        if (eskf.degraded) {
            flags.add("eskf_degraded")
        }
        if (eskf.localizationUncertaintyM > 6.0f) {
            flags.add("localization_uncertain")
        }
        if (depth.sourceCode != 1) {
            flags.add("depth_heuristic_fallback")
        }
        if (!latestVoApplied) {
            flags.add("vision_odometry_fallback")
        }
        val velocityMps = riskScore.coerceIn(0.2f, 3.0f)
        val sensorHealth = JSONObject()
            .put("score", eskf.sensorHealthScore.coerceIn(0.0f, 1.0f))
            .put("flags", flags)
            .put("degraded", eskf.degraded)
            .put("source", "android-eskf-runtime-v3")
        val sensorUncertainty = JSONObject()
            .put(
                "covariance_3x3",
                listOf(
                    eskf.covarianceXx, 0.0f, 0.0f,
                    0.0f, eskf.covarianceYy, 0.0f,
                    0.0f, 0.0f, eskf.covarianceZz,
                ),
            )
            .put("innovation_norm", eskf.innovationNorm.coerceIn(0.0f, 10.0f))
            .put("source", "android-eskf-runtime-v3")
        val visionOdometry = JSONObject()
            .put("source", if (latestVoApplied) "android-visual-odometry-v1" else "gps-anchor-fallback")
            .put("applied", latestVoApplied)
            .put("optical_flow_score", latestVoFlowScore.coerceIn(0.0f, 1.0f))
            .put("variance_m2", latestVoVarianceM2.coerceIn(0.0f, 999.0f))
            .put("pose_x_m", latestVoPoseXM)
            .put("pose_y_m", latestVoPoseYM)
        val cloudLink = JSONObject()
            .put("connected", wsOpen)
            .put("rtt_ms", JSONObject.NULL)
            .put("source", "android-live-ws-v1")
        val edgeSemanticCues = buildEdgeSemanticCues(depth)

        val payload = JSONObject()
            .put("type", "multimodal_frame")
            .put("session_id", sessionId)
            .put("timestamp", Instant.now().toString())
            .put("frame_jpeg_base64", JSONObject.NULL)
            .put("motion_state", "walking_fast")
            .put("pitch", 0.0)
            .put("velocity", velocityMps)
            .put("user_text", JSONObject.NULL)
            .put("yaw_delta_deg", 0.0)
            .put("carry_mode", "necklace")
            .put("sensor_unavailable", false)
            .put("lat", location?.lat ?: JSONObject.NULL)
            .put("lng", location?.lng ?: JSONObject.NULL)
            .put("heading_deg", JSONObject.NULL)
            .put("location_accuracy_m", location?.accuracyM ?: JSONObject.NULL)
            .put("location_age_ms", 0)
            .put("sensor_health", sensorHealth)
            .put("sensor_uncertainty", sensorUncertainty)
            .put("vision_odometry", visionOdometry)
            .put("cloud_link", cloudLink)
            .put("edge_semantic_cues", edgeSemanticCues)

        webSocket?.send(payload.toString())
    }

    private fun buildEdgeSemanticCues(depth: DepthHazardResult): org.json.JSONArray {
        val cues = org.json.JSONArray()
        if (!depth.detected) {
            return cues
        }

        val distanceM = when (depth.distanceCode) {
            0 -> 1.0f
            1 -> 2.5f
            2 -> 4.5f
            else -> 3.0f
        }
        val cue = JSONObject()
            .put("cue_type", "drop_ahead")
            .put("text", "Drop ahead")
            .put("confidence", depth.confidence.coerceIn(0.0f, 1.0f))
            .put("position_x", depth.positionX.coerceIn(-1.0f, 1.0f))
            .put("distance_m", distanceM)
            .put("position_clock", clockFaceFromPositionX(depth.positionX))
            .put("ttl_ms", 1200)
            .put("source", if (depth.sourceCode == 1) "edge_depth_onnx" else "edge_depth_heuristic")
        cues.put(cue)
        return cues
    }

    private fun clockFaceFromPositionX(positionX: Float): String {
        return when {
            positionX <= -0.6f -> "10h"
            positionX <= -0.25f -> "11h"
            positionX < 0.25f -> "12h"
            positionX < 0.6f -> "1h"
            else -> "2h"
        }
    }

    private fun sendAudioChunk(audioPcm16: ByteArray) {
        if (!running || !wsOpen) {
            return
        }
        val base64 = Base64.encodeToString(audioPcm16, Base64.NO_WRAP)
        val payload = JSONObject()
            .put("type", "audio_chunk")
            .put("session_id", sessionId)
            .put("timestamp", Instant.now().toString())
            .put("audio_chunk_pcm16", base64)
        webSocket?.send(payload.toString())
    }

    private fun sendHazardObservation(depth: DepthHazardResult, riskScore: Float) {
        if (!running || !wsOpen) {
            return
        }

        val distanceM = when (depth.distanceCode) {
            0 -> 1.0f
            1 -> 2.5f
            2 -> 4.5f
            else -> 3.0f
        }
        val relativeVelocityMps = -riskScore.coerceIn(0.4f, 3.0f)
        val source = if (depth.sourceCode == 1) "depth_onnx" else "depth_heuristic"

        val payload = JSONObject()
            .put("type", "hazard_observation")
            .put("session_id", sessionId)
            .put("timestamp", Instant.now().toString())
            .put("hazard_type", "DROP_AHEAD")
            .put("bearing_x", depth.positionX)
            .put("distance_m", distanceM)
            .put("relative_velocity_mps", relativeVelocityMps)
            .put("confidence", depth.confidence)
            .put("source", source)
            .put("suppress_ms", 3000)

        webSocket?.send(payload.toString())
    }

    private fun postJson(url: String, body: String): String {
        val request = Request.Builder()
            .url(url.trimEnd('/'))
            .post(body.toRequestBody("application/json".toMediaType()))
            .build()
        network.newCall(request).execute().use { response ->
            if (!response.isSuccessful) {
                throw IllegalStateException("HTTP ${response.code}: ${response.body?.string()}")
            }
            return response.body?.string().orEmpty()
        }
    }
}
