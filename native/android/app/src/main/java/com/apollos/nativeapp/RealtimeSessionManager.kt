package com.apollos.nativeapp

import android.Manifest
import android.content.Context
import android.content.pm.PackageManager
import android.graphics.Bitmap
import android.graphics.ImageFormat
import android.graphics.Rect
import android.graphics.YuvImage
import android.media.AudioAttributes
import android.media.AudioFormat
import android.media.AudioRecord
import android.media.AudioTrack
import android.media.MediaRecorder
import android.os.Build
import android.os.SystemClock
import java.io.ByteArrayOutputStream
import android.os.VibrationEffect
import android.os.Vibrator
import android.os.VibratorManager
import android.hardware.Sensor
import android.hardware.SensorEvent
import android.hardware.SensorEventListener
import android.hardware.SensorManager
import android.net.wifi.WifiManager
import android.net.nsd.NsdManager

import android.net.nsd.NsdServiceInfo
import android.util.Base64
import android.util.Log

import androidx.camera.core.CameraSelector
import androidx.camera.core.ImageAnalysis
import androidx.camera.core.ImageProxy
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
import kotlinx.coroutines.delay
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
import kotlin.math.atan2
import kotlin.math.cos
import kotlin.math.sin
import kotlin.math.sqrt
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow

data class LocationSnapshot(
    val lat: Double,
    val lng: Double,
    val accuracyM: Float,
)

data class TranscriptEntry(
    val id: String,
    val role: String, // assistant, user, system
    val text: String,
    val timestamp: String,
)

private data class AuthBootstrap(
    val baseUrl: String,
    val wsToken: String,
)

class RealtimeSessionManager(
    private val context: Context,
    private val lifecycleOwner: LifecycleOwner,
) {
    companion object {
        private const val SAFETY_DEADZONE_SCORE = 0.1f
        private const val HARD_STOP_THRESHOLD = 3.2f
        private const val SAFETY_TONE_SAMPLE_RATE = 16_000
        private const val SERVICE_TYPE = "_apollos._tcp."
        private const val RAD_TO_DEG = 57.29578f
        private const val CARRY_MODE_NECKLACE_CODE: Byte = 1
        private const val HAZARD_TYPE_DROP_AHEAD = 1
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
    @Volatile private var lastAccelTimestampNs: Long = 0L
    @Volatile private var lastGyroTimestampNs: Long = 0L
    @Volatile private var latestGyroMagnitudeDeg: Float = 0.0f
    @Volatile private var latestGyroAlphaDeg: Float = 0.0f
    @Volatile private var latestGyroBetaDeg: Float = 0.0f
    @Volatile private var latestGyroGammaDeg: Float = 0.0f
    @Volatile private var latestPitchDeg: Float = 0.0f
    @Volatile private var latestVelocityMps: Float = 0.0f
    @Volatile private var latestMotionStateCode: Byte = 0
    @Volatile private var latestSensorUnavailable: Boolean = true
    @Volatile private var latestKinematicResult: KinematicResult = KinematicResult(
        riskScore = 1.0f,
        shouldCapture = false,
        yawDeltaDeg = 0.0f,
    )
    @Volatile private var latestYawDeltaDeg: Float = 0.0f
    @Volatile private var hasAccelSample: Boolean = false
    @Volatile private var hasGyroSample: Boolean = false
    @Volatile private var latestVoApplied: Boolean = false
    @Volatile private var latestVoFlowScore: Float = 0.0f
    @Volatile private var latestVoVarianceM2: Float = 999.0f
    @Volatile private var latestVoPoseXM: Float = 0.0f
    @Volatile private var latestVoPoseYM: Float = 0.0f
    @Volatile private var latestDepthObjectsFeedAvailable: Boolean = false
    private var gravityEstimate = FloatArray(3)
    private val accelBuffer = FloatArray(3)
    private val edgePipeline by lazy {
        YoloDa3NnapiPipeline(context) { message ->
            Log.i("ApollosEdge", message)
        }
    }
    private var preview: androidx.camera.core.Preview? = null
    private var imageAnalysis: ImageAnalysis? = null
    private var surfaceProvider: androidx.camera.core.Preview.SurfaceProvider? = null

    private var sessionId: String = UUID.randomUUID().toString()
    private var webSocket: WebSocket? = null
    private var audioJob: Job? = null
    var running = false
        private set
    private var wsOpen = false
    private val lastFrameSentAtMs = AtomicLong(0L)
    @Volatile private var latestFrameBase64: String? = null
    private val lastJpegCompressionAtMs = AtomicLong(0L)
    private val safetyActuatorLock = Any()
    private var safetyToneTrack: AudioTrack? = null
    private var latestSafetyDirectiveAtMs: Long = 0L

    private val _navigationMode = MutableStateFlow("NAVIGATION")
    val navigationMode: StateFlow<String> = _navigationMode.asStateFlow()

    private val _hazardPosition = MutableStateFlow(0.0f)
    val hazardPosition: StateFlow<Float> = _hazardPosition.asStateFlow()

    private val _hazardVisible = MutableStateFlow(false)
    val hazardVisible: StateFlow<Boolean> = _hazardVisible.asStateFlow()

    private val _hazardDistance = MutableStateFlow("mid")
    val hazardDistance: StateFlow<String> = _hazardDistance.asStateFlow()

    private val _transcriptEntries = MutableStateFlow<List<TranscriptEntry>>(emptyList())
    val transcriptEntries: StateFlow<List<TranscriptEntry>> = _transcriptEntries.asStateFlow()

    private val _micActive = MutableStateFlow(false)
    val micActive: StateFlow<Boolean> = _micActive.asStateFlow()

    private val _wsStatus = MutableStateFlow("disconnected")
    val wsStatus: StateFlow<String> = _wsStatus.asStateFlow()

    private val _discoveredServerUrl = MutableStateFlow<String?>(null)
    val discoveredServerUrl: StateFlow<String?> = _discoveredServerUrl.asStateFlow()

    private var nsdManager: NsdManager? = context.getSystemService(Context.NSD_SERVICE) as? NsdManager
    private var discoveryListener: NsdManager.DiscoveryListener? = null
    private var multicastLock: WifiManager.MulticastLock? = null



    suspend fun start(
        serverBaseUrl: String,
        idToken: String,
        onStatus: (String) -> Unit,
    ): Boolean {
        if (running) return true
        sessionId = UUID.randomUUID().toString()
        val preferredBaseUrl = resolvePreferredServerBaseUrl(serverBaseUrl)

        if (!isHeadsetConnected()) {
            onStatus("ERROR: Headset required for safety.")
            addTranscript("system", "Session blocked: Please connect headphones (Bluetooth or Wired) to ensure private and clear safety cues.")
            running = false
            return false
        }
        
        val configStr = try {
            getJson("$preferredBaseUrl/config")
        } catch (e: Exception) {
            Log.e("Apollos", "Config fetch failed: ${e.message}")
            "{\"ws_auth_mode\":\"oidc_broker\"}" // Fallback to classic
        }
        val config = JSONObject(configStr)
        val authMode = config.optString("ws_auth_mode", "oidc_broker")
        
        var wsToken = "anonymous"
        var activeBaseUrl = preferredBaseUrl
        if (authMode != "disabled") {
            try {
                onStatus("Fetching tokens from $preferredBaseUrl...")
                val auth = fetchAuthTokensWithFallback(preferredBaseUrl, idToken, onStatus)
                activeBaseUrl = auth.baseUrl
                wsToken = auth.wsToken
            } catch (error: Throwable) {
                Log.e("Apollos", "Auth failed: ${error.message}")
                onStatus("Auth failed: ${error.message}")
                return false
            }
        } else {
            Log.i("Apollos", "Using anonymode (ws_auth_mode disabled)")
            onStatus("Auth disabled (Guest mode)")
        }
        
        running = true
        Log.i("Apollos", "Session initiation starting...")
        onStatus("Auth OK")
        eskfHandle = RustCoreBridge.eskfCreate()
        if (eskfHandle == 0L) {
            running = false
            onStatus("ESKF create failed")
            return false
        }
        geoOrigin = null
        lastImuTimestampNs = 0L
        lastAccelTimestampNs = 0L
        lastGyroTimestampNs = 0L
        latestGyroMagnitudeDeg = 0.0f
        latestGyroAlphaDeg = 0.0f
        latestGyroBetaDeg = 0.0f
        latestGyroGammaDeg = 0.0f
        latestPitchDeg = 0.0f
        latestVelocityMps = 0.0f
        latestMotionStateCode = 0
        latestSensorUnavailable = true
        latestKinematicResult = KinematicResult(
            riskScore = 1.0f,
            shouldCapture = false,
            yawDeltaDeg = 0.0f,
        )
        latestYawDeltaDeg = 0.0f
        hasAccelSample = false
        hasGyroSample = false
        latestVoApplied = false
        latestVoFlowScore = 0.0f
        latestVoVarianceM2 = 999.0f
        latestVoPoseXM = 0.0f
        latestVoPoseYM = 0.0f
        latestDepthObjectsFeedAvailable = false
        gravityEstimate = FloatArray(3)
        latestSafetyDirectiveAtMs = 0L
        warmEdgePipeline(onStatus)

        connectLiveSocket(activeBaseUrl, wsToken, onStatus)

        var retry = 0
        while (!wsOpen && retry < 20) {
            delay(200)
            retry++
        }
        if (!wsOpen) {
            onStatus("Connection timed out")
            running = false
            webSocket?.cancel()
            webSocket = null
            _wsStatus.value = "failure"
            if (eskfHandle != 0L) {
                RustCoreBridge.eskfDestroy(eskfHandle)
                eskfHandle = 0L
            }
            return false
        }

        startLocation(onStatus)
        startSensors(onStatus)
        startAudio(onStatus)
        startCamera(onStatus)
        _wsStatus.value = "connected"
        Log.i("Apollos", "Data loops started after WS handshake")
        onStatus("Session active")
        return true
    }

    fun startDiscovery() {
        if (discoveryListener != null) return
        
        val wifi = context.applicationContext.getSystemService(Context.WIFI_SERVICE) as? WifiManager
        multicastLock = wifi?.createMulticastLock("ApollosDiscoveryLock")
        multicastLock?.setReferenceCounted(true)
        multicastLock?.acquire()

        discoveryListener = object : NsdManager.DiscoveryListener {

            override fun onDiscoveryStarted(regType: String) {
                Log.d("Apollos", "Service discovery started")
            }

            override fun onServiceFound(service: NsdServiceInfo) {
                Log.d("Apollos", "Service found: ${service.serviceName} type: ${service.serviceType}")
                if (service.serviceType.contains("apollos", ignoreCase = true)) {
                    Log.i("Apollos", "Found Apollos service, resolving...")
                    nsdManager?.resolveService(service, object : NsdManager.ResolveListener {

                        override fun onResolveFailed(serviceInfo: NsdServiceInfo, errorCode: Int) {
                            Log.e("Apollos", "Resolve failed: $errorCode")
                        }

                        override fun onServiceResolved(serviceInfo: NsdServiceInfo) {
                            val host = serviceInfo.host.hostAddress
                            val port = serviceInfo.port
                            val url = "http://$host:$port"
                            Log.i("Apollos", "Resolved server: $url")
                            _discoveredServerUrl.value = url
                        }
                    })
                }
            }

            override fun onServiceLost(service: NsdServiceInfo) {
                Log.d("Apollos", "Service lost: ${service.serviceName}")
            }

            override fun onDiscoveryStopped(regType: String) {
                Log.d("Apollos", "Discovery stopped")
            }

            override fun onStartDiscoveryFailed(serviceType: String, errorCode: Int) {
                Log.e("Apollos", "Discovery failed: $errorCode")
                nsdManager?.stopServiceDiscovery(this)
            }

            override fun onStopDiscoveryFailed(serviceType: String, errorCode: Int) {
                Log.e("Apollos", "Stop discovery failed: $errorCode")
                nsdManager?.stopServiceDiscovery(this)
            }
        }

        nsdManager?.discoverServices(SERVICE_TYPE, NsdManager.PROTOCOL_DNS_SD, discoveryListener)
    }

    fun stopDiscovery() {
        discoveryListener?.let {
            nsdManager?.stopServiceDiscovery(it)
        }
        discoveryListener = null
        multicastLock?.release()
        multicastLock = null
    }



    fun setMicActive(active: Boolean) {
        if (_micActive.value == active) return
        _micActive.value = active
        addTranscript("system", "Microphone ${if (active) "ON" else "OFF"}")
    }

    suspend fun stop(onStatus: (String) -> Unit = {}) {
        val wasRunning = running
        running = false

        webSocket?.close(1000, "client_stop")
        webSocket = null
        wsOpen = false
        _wsStatus.value = "disconnected"

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
        lastAccelTimestampNs = 0L
        lastGyroTimestampNs = 0L
        latestGyroMagnitudeDeg = 0.0f
        latestGyroAlphaDeg = 0.0f
        latestGyroBetaDeg = 0.0f
        latestGyroGammaDeg = 0.0f
        latestPitchDeg = 0.0f
        latestVelocityMps = 0.0f
        latestMotionStateCode = 0
        latestSensorUnavailable = true
        latestKinematicResult = KinematicResult(
            riskScore = 1.0f,
            shouldCapture = false,
            yawDeltaDeg = 0.0f,
        )
        latestYawDeltaDeg = 0.0f
        hasAccelSample = false
        hasGyroSample = false
        latestVoApplied = false
        latestVoFlowScore = 0.0f
        latestVoVarianceM2 = 999.0f
        latestVoPoseXM = 0.0f
        latestVoPoseYM = 0.0f
        latestDepthObjectsFeedAvailable = false
        latestSafetyDirectiveAtMs = 0L
        if (edgePipeline.isAvailable()) {
            edgePipeline.close()
        }
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
        if (wasRunning) {
            onStatus("Stopped")
        }
    }

    private suspend fun getJson(url: String): String = withContext(Dispatchers.IO) {
        val request = Request.Builder().url(url).build()
        network.newCall(request).execute().use { response ->
            if (!response.isSuccessful) throw Exception("GET $url failed: ${response.code}")
            response.body?.string() ?: ""
        }
    }

    private suspend fun fetchAuthTokens(
        serverBaseUrl: String,
        idToken: String,
    ): AuthBootstrap = withContext(Dispatchers.IO) {
        val normalizedBaseUrl = normalizeBaseUrl(serverBaseUrl)
        val exchange = JSONObject()
            .put("id_token", idToken.trim())
            .toString()
        val exchangeBody = postJson("$normalizedBaseUrl/auth/oidc/exchange", exchange)
        val sessionToken = JSONObject(exchangeBody).getString("session_token")

        val wsReq = JSONObject().put("session_token", sessionToken).toString()
        val wsBody = postJson("$normalizedBaseUrl/auth/ws-ticket", wsReq)
        val wsToken = JSONObject(wsBody).getString("access_token")
        AuthBootstrap(
            baseUrl = normalizedBaseUrl,
            wsToken = wsToken,
        )
    }

    private suspend fun fetchAuthTokensWithFallback(
        serverBaseUrl: String,
        idToken: String,
        onStatus: (String) -> Unit,
    ): AuthBootstrap {
        val primaryBaseUrl = normalizeBaseUrl(serverBaseUrl)
        return try {
            fetchAuthTokens(primaryBaseUrl, idToken)
        } catch (primaryError: Exception) {
            val discoveredBaseUrl = discoveredServerUrl.value?.let(::normalizeBaseUrl)
            val shouldRetryOnDiscovery = primaryError.message?.contains("HTTP 404") == true &&
                !discoveredBaseUrl.isNullOrBlank() &&
                discoveredBaseUrl != primaryBaseUrl
            if (!shouldRetryOnDiscovery) {
                throw primaryError
            }

            Log.w(
                "Apollos",
                "Auth returned 404 on $primaryBaseUrl, retrying discovered server $discoveredBaseUrl",
            )
            onStatus("Auth 404 on $primaryBaseUrl, retrying discovered server...")
            fetchAuthTokens(discoveredBaseUrl, idToken)
        }
    }

    private fun resolvePreferredServerBaseUrl(configuredBaseUrl: String): String {
        val normalizedConfigured = normalizeBaseUrl(configuredBaseUrl)
        val discovered = discoveredServerUrl.value?.let(::normalizeBaseUrl)
        return if (!discovered.isNullOrBlank() && isDefaultConfiguredBaseUrl(normalizedConfigured)) {
            discovered
        } else {
            normalizedConfigured
        }
    }

    private fun isDefaultConfiguredBaseUrl(baseUrl: String): Boolean {
        return baseUrl == normalizeBaseUrl(BuildConfig.SERVER_URL)
    }

    private fun normalizeBaseUrl(baseUrl: String): String {
        return baseUrl.trim().trimEnd('/')
    }

    private fun connectLiveSocket(serverBaseUrl: String, wsToken: String, onStatus: (String) -> Unit) {
        val wsBase = serverBaseUrl
            .replaceFirst("https://", "wss://")
            .replaceFirst("http://", "ws://")
            .trimEnd('/')
        
        val requestBuilder = Request.Builder()
            .url("$wsBase/ws/live/$sessionId")
        
        if (wsToken != "anonymous") {
            val tokenB64 = Base64.encodeToString(
                wsToken.toByteArray(Charsets.UTF_8),
                Base64.URL_SAFE or Base64.NO_WRAP or Base64.NO_PADDING,
            )
            val protocolHeader = "authb64.$tokenB64, apollos.v1"
            requestBuilder.header("Sec-WebSocket-Protocol", protocolHeader)
        } else {
            requestBuilder.header("Sec-WebSocket-Protocol", "apollos.v1")
        }

        val request = requestBuilder.build()

        webSocket = network.newWebSocket(request, object : WebSocketListener() {
            override fun onOpen(webSocket: WebSocket, response: Response) {
                wsOpen = true
                _wsStatus.value = "connected"
                Log.i("Apollos", "WebSocket connected successfully to $wsBase")
                onStatus("WS connected")
            }

            override fun onMessage(webSocket: WebSocket, text: String) {
                // Log.d("Apollos", "WS Msg: $text") // Verbose
                handleServerMessage(text, onStatus)
            }

            override fun onClosing(webSocket: WebSocket, code: Int, reason: String) {
                _wsStatus.value = "closing"
                Log.w("Apollos", "WebSocket closing: $code / $reason")
                onStatus("WS closing: $code $reason")
            }

            override fun onClosed(webSocket: WebSocket, code: Int, reason: String) {
                wsOpen = false
                _wsStatus.value = "disconnected"
                Log.w("Apollos", "WebSocket closed: $code")
                onStatus("WS closed: $code")
            }

            override fun onFailure(webSocket: WebSocket, t: Throwable, response: Response?) {
                wsOpen = false
                _wsStatus.value = "failure"
                Log.e("Apollos", "WebSocket failure: ${t.message}", t)
                onStatus("WS failure: ${t.message}")
            }
        })
        _wsStatus.value = "connecting"
    }

    private fun addTranscript(role: String, text: String) {
        val entry = TranscriptEntry(
            id = UUID.randomUUID().toString(),
            role = role,
            text = text,
            timestamp = Instant.now().toString()
        )
        val current = _transcriptEntries.value.toMutableList()
        current.add(entry)
        if (current.size > 200) current.removeAt(0)
        _transcriptEntries.value = current
    }

    fun setNavigationMode(mode: String) {
        _navigationMode.value = mode
        addTranscript("system", "Mode switched to $mode")
        // In a real app we'd send a command to backend here
        val payload = JSONObject()
            .put("type", "user_command")
            .put("session_id", sessionId)
            .put("timestamp_ms", Instant.now().toEpochMilli())
            .put("command", "set_navigation_mode:$mode")
        webSocket?.send(payload.toString())
    }

    fun cycleNavigationMode() {
        val modes = listOf("NAVIGATION", "EXPLORE", "READ", "QUIET")
        val currentIndex = modes.indexOf(_navigationMode.value)
        val nextIndex = (currentIndex + 1) % modes.size
        setNavigationMode(modes[nextIndex])
    }

    fun toggleMic() {
        setMicActive(!_micActive.value)
    }

    fun requestDetailedDescription() {
        addTranscript("user", "Describe in detail requested")
        val payload = JSONObject()
            .put("type", "user_command")
            .put("session_id", sessionId)
            .put("timestamp_ms", Instant.now().toEpochMilli())
            .put("command", "describe_detailed")
        webSocket?.send(payload.toString())
    }

    fun requestHumanHelp() {
        addTranscript("user", "Human help requested")
        val payload = JSONObject()
            .put("type", "user_command")
            .put("session_id", sessionId)
            .put("timestamp_ms", Instant.now().toEpochMilli())
            .put("command", "request_human_help")
        webSocket?.send(payload.toString())
    }

    private fun handleServerMessage(text: String, onStatus: (String) -> Unit) {
        val payload = runCatching { JSONObject(text) }.getOrNull()
        if (payload == null) {
            onStatus("WS non-json message: ${text.take(80)}")
            return
        }

        when (payload.optString("type")) {
            "safety_directive" -> {
                applySafetyDirective(payload, onStatus)
                val hazardScore = payload.optDouble("hazard_score", 0.0).toFloat()
                val hardStop = payload.optBoolean("hard_stop", false)
                if (hardStop || hazardScore > 0.5f) {
                    _hazardVisible.value = true
                    _hazardPosition.value = payload.optDouble("position_x", 0.0).toFloat()
                    _hazardDistance.value = if (hazardScore > 0.8) "very_close" else "mid"
                } else {
                    _hazardVisible.value = false
                }
            }
            "assistant_text" -> {
                val text = payload.optString("text")
                addTranscript("assistant", text)
                onStatus("Assistant: $text")
            }
            "hard_stop" -> {
                _hazardVisible.value = true
                _hazardPosition.value = payload.optDouble("position_x", 0.0).toFloat()
                _hazardDistance.value = payload.optString("distance", "mid")
                val type = payload.optString("hazard_type", "unknown")
                addTranscript("system", "STOP: $type")
                onStatus("HARD STOP: $type")
            }
            "connection_state" -> {
                val state = payload.optString("state")
                _wsStatus.value = state
                val detail = payload.optString("detail").ifBlank { "state_update" }
                onStatus("WS state: $detail")
            }
            "cognition_state" -> {
                val layer = payload.optString("active_layer").ifBlank { "unknown_layer" }
                val reason = payload.optString("reason").ifBlank { "no_reason" }
                onStatus("WS cognition: $layer ($reason)")
                addTranscript("system", "Cognition: $layer ($reason)")
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

    private fun triggerSafetyActuator(hazardScore: Float, hardStop: Boolean) {
        val nowMs = SystemClock.elapsedRealtime()
        if ((nowMs - latestSafetyDirectiveAtMs) < 60) {
            return
        }
        latestSafetyDirectiveAtMs = nowMs

        // Math matching safety_policy.rs: H(d, v) mapped to local actuators
        val activation = ((hazardScore - SAFETY_DEADZONE_SCORE) / (HARD_STOP_THRESHOLD + 1.0f)).coerceIn(0.0f, 1.0f)
        val hapticIntensity = Math.pow(activation.toDouble(), 0.75).toFloat()
        val pitchHz = if (activation <= 0.0f) 0.0f else (330.0f + activation * 770.0f)

        if (hazardScore < SAFETY_DEADZONE_SCORE || pitchHz <= 0.0f || hapticIntensity <= 0.0f) {
            stopSafetyActuator()
            return
        }

        val durationMs = if (hardStop) 220 else 90
        val gain = hapticIntensity.coerceIn(0.08f, 1.0f)
        playSafetyTone(pitchHz, gain, durationMs, { /* no-op */ })
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
                            .setUsage(AudioAttributes.USAGE_ASSISTANCE_ACCESSIBILITY)
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

        val h = intensity.coerceIn(0.0f, 1.0f)
        val amplitude = (h * 255.0f).toInt().coerceIn(1, 255)
        val normalizedDurationMs = durationMs.coerceAtLeast(40)

        val (timings, amplitudes) = when {
            h >= 0.8f -> {
                longArrayOf(0, 25, 25, 25, 25, 25) to intArrayOf(0, amplitude, 0, amplitude, 0, amplitude)
            }
            h >= 0.5f -> {
                longArrayOf(
                    0,
                    (normalizedDurationMs / 2).toLong(),
                    60,
                    (normalizedDurationMs / 2).toLong(),
                    60,
                    (normalizedDurationMs / 2).toLong(),
                ) to intArrayOf(0, amplitude, 0, amplitude, 0, amplitude)
            }
            else -> {
                longArrayOf(0, normalizedDurationMs.toLong(), 180, normalizedDurationMs.toLong()) to
                    intArrayOf(0, amplitude, 0, amplitude)
            }
        }
        val effect = VibrationEffect.createWaveform(timings, amplitudes, -1)
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

        val linearAccelSensor = manager.getDefaultSensor(Sensor.TYPE_LINEAR_ACCELERATION)
        val gravitySensor = manager.getDefaultSensor(Sensor.TYPE_GRAVITY)
        val gyroSensor = manager.getDefaultSensor(Sensor.TYPE_GYROSCOPE)

        if (linearAccelSensor == null && gravitySensor == null && gyroSensor == null) {
            onStatus("IMU sensors unavailable")
            return
        }
        if (linearAccelSensor == null) {
            onStatus("Linear acceleration unavailable; ESKF prediction disabled")
        }

        val listener = object : SensorEventListener {
            override fun onSensorChanged(event: SensorEvent) {
                if (!running) {
                    return
                }

                when (event.sensor.type) {
                    Sensor.TYPE_LINEAR_ACCELERATION -> {
                        val timestampNs = event.timestamp
                        val dtS = if (lastAccelTimestampNs == 0L) {
                            0.0f
                        } else {
                            ((timestampNs - lastAccelTimestampNs).coerceAtLeast(0L) / 1_000_000_000f)
                        }
                        lastAccelTimestampNs = timestampNs
                        lastImuTimestampNs = timestampNs

                        accelBuffer[0] = event.values[0]
                        accelBuffer[1] = event.values[1]
                        accelBuffer[2] = event.values[2]

                        hasAccelSample = true
                        latestSensorUnavailable = !(hasAccelSample && hasGyroSample)
                        if (dtS in 1.0e-4f..0.2f) {
                            RustCoreBridge.eskfPredictImu(
                                handle = eskfHandle,
                                accelX = accelBuffer[0],
                                accelY = accelBuffer[1],
                                accelZ = accelBuffer[2],
                                dtS = dtS,
                            )
                            val accelMagnitude = sqrt(
                                accelBuffer[0] * accelBuffer[0] +
                                    accelBuffer[1] * accelBuffer[1] +
                                    accelBuffer[2] * accelBuffer[2],
                            )
                            val damping = (1.0f - dtS * 1.6f).coerceIn(0.0f, 1.0f)
                            latestVelocityMps =
                                (latestVelocityMps * damping + accelMagnitude * dtS).coerceIn(0.0f, 4.5f)
                        }
                        pushLatestKinematicToRust()
                    }
                    Sensor.TYPE_GRAVITY -> {
                        gravityEstimate[0] = event.values.getOrElse(0) { 0.0f }
                        gravityEstimate[1] = event.values.getOrElse(1) { 0.0f }
                        gravityEstimate[2] = event.values.getOrElse(2) { 0.0f }
                        updatePitchFromGravity()
                    }
                    Sensor.TYPE_GYROSCOPE -> {
                        val timestampNs = event.timestamp
                        val dtMs = if (lastGyroTimestampNs == 0L) {
                            0.0f
                        } else {
                            ((timestampNs - lastGyroTimestampNs).coerceAtLeast(0L) / 1_000_000f)
                        }
                        lastGyroTimestampNs = timestampNs
                        lastImuTimestampNs = timestampNs

                        val xDeg = event.values.getOrElse(0) { 0.0f } * RAD_TO_DEG
                        val yDeg = event.values.getOrElse(1) { 0.0f } * RAD_TO_DEG
                        val zDeg = event.values.getOrElse(2) { 0.0f } * RAD_TO_DEG
                        latestGyroAlphaDeg = xDeg
                        latestGyroBetaDeg = yDeg
                        latestGyroGammaDeg = zDeg
                        latestGyroMagnitudeDeg = sqrt(xDeg * xDeg + yDeg * yDeg + zDeg * zDeg)
                        if (dtMs in 0.1f..200.0f) {
                            latestYawDeltaDeg = RustCoreBridge.computeYawDelta(xDeg, dtMs)
                        }
                        hasGyroSample = true
                        latestSensorUnavailable = !(hasAccelSample && hasGyroSample)
                        pushLatestKinematicToRust()
                    }
                }
            }

            override fun onAccuracyChanged(sensor: Sensor?, accuracy: Int) = Unit
        }

        sensorListener = listener
        linearAccelSensor?.let {
            manager.registerListener(listener, it, SensorManager.SENSOR_DELAY_FASTEST)
        }
        gravitySensor?.let {
            manager.registerListener(listener, it, SensorManager.SENSOR_DELAY_FASTEST)
        }
        gyroSensor?.let {
            manager.registerListener(listener, it, SensorManager.SENSOR_DELAY_FASTEST)
        }
        onStatus("IMU stream started")
    }

    private fun updatePitchFromGravity() {
        val gx = gravityEstimate[0]
        val gy = gravityEstimate[1]
        val gz = gravityEstimate[2]
        val horizontalMagnitude = sqrt(gy * gy + gz * gz)
        if (!gx.isFinite() || !horizontalMagnitude.isFinite()) {
            return
        }
        latestPitchDeg = Math.toDegrees(
            atan2((-gx).toDouble(), horizontalMagnitude.toDouble()),
        ).toFloat()
    }

    private fun pushLatestKinematicToRust() {
        latestMotionStateCode = motionStateCodeFromVelocity(latestVelocityMps)
        latestKinematicResult = RustCoreBridge.analyzeKinematics(
            motionStateCode = latestMotionStateCode,
            carryModeCode = CARRY_MODE_NECKLACE_CODE,
            pitch = latestPitchDeg,
            velocity = latestVelocityMps,
            yawDeltaDeg = latestYawDeltaDeg,
            accelX = accelBuffer[0],
            accelY = accelBuffer[1],
            accelZ = accelBuffer[2],
            gyroAlpha = latestGyroAlphaDeg,
            gyroBeta = latestGyroBetaDeg,
            gyroGamma = latestGyroGammaDeg,
            sensorUnavailable = latestSensorUnavailable,
        )
    }

    private fun motionStateCodeFromVelocity(velocityMps: Float): Byte {
        return when {
            velocityMps >= 2.6f -> 3
            velocityMps >= 0.8f -> 2
            velocityMps >= 0.15f -> 1
            else -> 0
        }
    }

    private fun motionStateLabel(code: Byte): String {
        return when (code.toInt()) {
            3 -> "running"
            2 -> "walking_fast"
            1 -> "walking_slow"
            else -> "stationary"
        }
    }

    private fun carryModeLabel(code: Byte): String {
        return when (code.toInt()) {
            0 -> "hand_held"
            2 -> "chest_clip"
            3 -> "pocket"
            else -> "necklace"
        }
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

        val applied = RustCoreBridge.eskfUpdateVision(
            handle = eskfHandle,
            positionX = posX,
            positionY = posY,
            positionZ = 0.0f,
            varianceM2 = varianceM2,
        )
        latestVoApplied = applied
        latestVoFlowScore = if (applied) 1.0f else 0.0f
        latestVoVarianceM2 = varianceM2
        latestVoPoseXM = posX
        latestVoPoseYM = posY
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
                    if (!running || !wsOpen) {
                        kotlinx.coroutines.delay(100)
                        continue
                    }
                    
                    val read = recorder.read(buffer, 0, buffer.size)
                    if (read <= 0) continue
                    
                    // Simple VAD: Calculate RMS (Root Mean Square) for PCM 16-bit
                    var sum = 0.0
                    for (i in 0 until read step 2) {
                        val sample = (buffer[i].toInt() and 0xFF) or (buffer[i + 1].toInt() shl 8)
                        sum += sample * sample
                    }
                    val rms = Math.sqrt(sum / (read / 2))
                    
                    // VAD Threshold: 500 (Heuristic for speech/activity vs silence)
                    if (rms > 500.0) {
                        sendAudioChunk(buffer.copyOf(read))
                    }
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
        logEdgePipelineStatus("before CameraX bind")

        val cameraProviderFuture = ProcessCameraProvider.getInstance(context)
        cameraProviderFuture.addListener(
            {
                val provider = cameraProviderFuture.get()
                
                preview = androidx.camera.core.Preview.Builder().build()
                surfaceProvider?.let { preview?.setSurfaceProvider(it) }
                
                val analysis = ImageAnalysis.Builder()
                    .setBackpressureStrategy(ImageAnalysis.STRATEGY_KEEP_ONLY_LATEST)
                    .setOutputImageFormat(ImageAnalysis.OUTPUT_IMAGE_FORMAT_RGBA_8888)
                    .build()
                imageAnalysis = analysis
                
                analysis.setAnalyzer(cameraExecutor) { image ->
                    if (!running || !wsOpen) {
                        image.close()
                        return@setAnalyzer
                    }
                    val nowMs = SystemClock.elapsedRealtime()
                    if (nowMs - lastJpegCompressionAtMs.get() > 1000) {
                        lastJpegCompressionAtMs.set(nowMs)
                        latestFrameBase64 = encodeImageToJpegB64(image)
                    }

                    val kinematic = latestKinematicResult
                    
                    // Local Safety Reflex: Protect user immediately if H > 3.2
                    if (kinematic.riskScore >= HARD_STOP_THRESHOLD) {
                        triggerSafetyActuator(kinematic.riskScore, true)
                    }
                    val detections = detectEdgeObjects(image)
                    val depth = RustCoreBridge.detectDropAheadObjects(
                        objects = detections,
                        riskScore = kinematic.riskScore,
                        carryModeCode = CARRY_MODE_NECKLACE_CODE,
                        gyroMagnitude = latestGyroMagnitudeDeg,
                        nowMs = SystemClock.elapsedRealtime(),
                    )
                    if (depth.detected) {
                        // Immediate Edge-side (Layer 1) hazard feedback
                        _hazardVisible.value = true
                        _hazardPosition.value = depth.positionX
                        _hazardDistance.value = when(depth.distanceCode) {
                            0 -> "very_close"
                            1 -> "mid"
                            else -> "far"
                        }
                        sendHazardObservation(depth)
                    }
                    sendMultimodalFrame(kinematic.riskScore, depth)
                    image.close()
                }

                provider.unbindAll()
                provider.bindToLifecycle(
                    lifecycleOwner,
                    CameraSelector.DEFAULT_BACK_CAMERA,
                    preview,
                    analysis,
                )
            },
            ContextCompat.getMainExecutor(context),
        )
    }

    private fun warmEdgePipeline(onStatus: (String) -> Unit) {
        logEdgePipelineStatus("during session start")
        if (!edgePipeline.isAvailable()) {
            onStatus("Edge models unavailable: ${edgePipeline.unavailableReason() ?: "unknown"}")
        }
    }

    fun debugSmokeCheckEdgePipeline() {
        if (!BuildConfig.DEBUG) {
            return
        }
        logEdgePipelineStatus("during debug service startup")
    }

    private fun logEdgePipelineStatus(stage: String) {
        if (edgePipeline.isAvailable()) {
            Log.i("ApollosEdge", "Smoke check: edge pipeline ready $stage")
        } else {
            Log.e(
                "ApollosEdge",
                "Smoke check: edge pipeline unavailable $stage: ${edgePipeline.unavailableReason() ?: "unknown"}",
            )
        }
    }

    fun setSurfaceProvider(surfaceProvider: androidx.camera.core.Preview.SurfaceProvider) {
        this.surfaceProvider = surfaceProvider
        preview?.setSurfaceProvider(surfaceProvider)
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
        if (!latestDepthObjectsFeedAvailable) {
            flags.add("depth_objects_feed_missing")
        }
        if (!latestVoApplied) {
            flags.add("vision_odometry_fallback")
        }
        val velocityMps = latestVelocityMps.coerceIn(0.0f, 4.5f)
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
            .put("timestamp_ms", Instant.now().toEpochMilli())
            .put("frame_jpeg_base64", latestFrameBase64 ?: JSONObject.NULL)
            .put("motion_state", motionStateLabel(latestMotionStateCode))
            .put("pitch", latestPitchDeg)
            .put("velocity", velocityMps)
            .put("user_text", JSONObject.NULL)
            .put("yaw_delta_deg", latestYawDeltaDeg)
            .put("carry_mode", carryModeLabel(CARRY_MODE_NECKLACE_CODE))
            .put("sensor_unavailable", latestSensorUnavailable)
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
        latestFrameBase64 = null
        webSocket?.send(payload.toString())
    }

    private fun encodeImageToJpegB64(image: ImageProxy): String? {
        val bitmap = try {
            val width = image.width
            val height = image.height
            val planes = image.planes
            val buffer = planes[0].buffer
            val pixelStride = planes[0].pixelStride
            val rowStride = planes[0].rowStride
            val rowPadding = rowStride - pixelStride * width
            
            val bitmap = Bitmap.createBitmap(
                width + rowPadding / pixelStride,
                height,
                Bitmap.Config.ARGB_8888
            )
            bitmap.copyPixelsFromBuffer(buffer)
            
            val baseBitmap = bitmap
            val rotation = image.imageInfo.rotationDegrees
            if (rotation != 0) {
                val matrix = android.graphics.Matrix()
                matrix.postRotate(rotation.toFloat())
                Bitmap.createBitmap(
                    baseBitmap, 0, 0, width, height, matrix, true
                )
            } else if (rowPadding != 0) {
                Bitmap.createBitmap(baseBitmap, 0, 0, width, height)
            } else {
                baseBitmap
            }
        } catch (e: Exception) {
            null
        } ?: return null

        val out = ByteArrayOutputStream()
        bitmap.compress(Bitmap.CompressFormat.JPEG, 70, out)
        val bytes = out.toByteArray()
        return Base64.encodeToString(bytes, Base64.NO_WRAP)
    }

    private fun detectEdgeObjects(image: ImageProxy): List<EdgeObjectDetection> {
        val inference = edgePipeline.detect(image)
        latestDepthObjectsFeedAvailable = inference.feedAvailable
        return inference.objects
    }

    private fun buildEdgeSemanticCues(depth: DepthHazardResult): org.json.JSONArray {
        val cues = org.json.JSONArray()
        if (!depth.detected) {
            return cues
        }

        val cue = JSONObject()
            .put("cue_type", "drop_ahead")
            .put("text", "Drop ahead")
            .put("confidence", depth.confidence.coerceIn(0.0f, 1.0f))
            .put("position_x", depth.positionX.coerceIn(-1.0f, 1.0f))
            .put("distance_m", depth.distanceM.coerceAtLeast(0.0f))
            .put("position_clock", clockFaceFromPositionX(depth.positionX))
            .put("ttl_ms", 1200)
            .put("source", "edge_depth_objects_v4_ttc")
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
            .put("timestamp_ms", Instant.now().toEpochMilli())
            .put("audio_chunk_pcm16", base64)
        webSocket?.send(payload.toString())
    }

    private fun sendHazardObservation(depth: DepthHazardResult) {
        if (!running || !wsOpen) {
            return
        }

        val distanceM = depth.distanceM.coerceAtLeast(0.0f)
        val relativeVelocityMps = depth.relativeVelocityMps
        val source = "depth_objects_v4_ttc"

        val payload = JSONObject()
            .put("type", "hazard_observation")
            .put("session_id", sessionId)
            .put("timestamp_ms", Instant.now().toEpochMilli())
            .put("hazard_type", HAZARD_TYPE_DROP_AHEAD)
            .put("bearing_x", depth.positionX)
            .put("distance_m", distanceM)
            .put("relative_velocity_mps", relativeVelocityMps)
            .put("time_to_collision_s", depth.timeToCollisionS ?: JSONObject.NULL)
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
                throw IllegalStateException(
                    "POST $url failed: HTTP ${response.code}: ${response.body?.string().orEmpty()}",
                )
            }
            return response.body?.string().orEmpty()
        }
    }

    private fun isHeadsetConnected(): Boolean {
        val audioManager = context.getSystemService(Context.AUDIO_SERVICE) as? android.media.AudioManager
            ?: return false
        
        val devices = audioManager.getDevices(android.media.AudioManager.GET_DEVICES_OUTPUTS)

        for (device in devices) {
            when (device.type) {
                android.media.AudioDeviceInfo.TYPE_WIRED_HEADSET,
                android.media.AudioDeviceInfo.TYPE_WIRED_HEADPHONES,
                android.media.AudioDeviceInfo.TYPE_BLUETOOTH_A2DP,
                android.media.AudioDeviceInfo.TYPE_BLUETOOTH_SCO,
                android.media.AudioDeviceInfo.TYPE_USB_HEADSET,
                android.media.AudioDeviceInfo.TYPE_HEARING_AID,
                android.media.AudioDeviceInfo.TYPE_BLE_HEADSET -> return true
                android.media.AudioDeviceInfo.TYPE_AUX_LINE,
                android.media.AudioDeviceInfo.TYPE_BLE_BROADCAST,
                android.media.AudioDeviceInfo.TYPE_BLE_SPEAKER,
                android.media.AudioDeviceInfo.TYPE_BUILTIN_EARPIECE,
                android.media.AudioDeviceInfo.TYPE_BUILTIN_MIC,
                android.media.AudioDeviceInfo.TYPE_BUILTIN_SPEAKER,
                android.media.AudioDeviceInfo.TYPE_BUILTIN_SPEAKER_SAFE,
                android.media.AudioDeviceInfo.TYPE_BUS,
                android.media.AudioDeviceInfo.TYPE_DOCK,
                android.media.AudioDeviceInfo.TYPE_DOCK_ANALOG,
                android.media.AudioDeviceInfo.TYPE_FM,
                android.media.AudioDeviceInfo.TYPE_FM_TUNER,
                android.media.AudioDeviceInfo.TYPE_HDMI,
                android.media.AudioDeviceInfo.TYPE_HDMI_ARC,
                android.media.AudioDeviceInfo.TYPE_HDMI_EARC,
                android.media.AudioDeviceInfo.TYPE_IP,
                android.media.AudioDeviceInfo.TYPE_LINE_ANALOG,
                android.media.AudioDeviceInfo.TYPE_LINE_DIGITAL,
                android.media.AudioDeviceInfo.TYPE_REMOTE_SUBMIX,
                android.media.AudioDeviceInfo.TYPE_TELEPHONY,
                android.media.AudioDeviceInfo.TYPE_TV_TUNER,
                android.media.AudioDeviceInfo.TYPE_USB_ACCESSORY,
                android.media.AudioDeviceInfo.TYPE_USB_DEVICE -> Unit
                else -> Unit
            }
        }
        return false
    }
}
