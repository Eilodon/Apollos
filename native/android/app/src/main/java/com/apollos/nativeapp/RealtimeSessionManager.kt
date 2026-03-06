package com.apollos.nativeapp

import android.Manifest
import android.content.Context
import android.content.pm.PackageManager
import android.media.AudioFormat
import android.media.AudioRecord
import android.media.MediaRecorder
import android.os.SystemClock
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

data class LocationSnapshot(
    val lat: Double,
    val lng: Double,
    val accuracyM: Float,
)

class RealtimeSessionManager(
    private val context: Context,
    private val lifecycleOwner: LifecycleOwner,
) {
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Main.immediate)
    private val network = OkHttpClient.Builder()
        .connectTimeout(12, TimeUnit.SECONDS)
        .readTimeout(0, TimeUnit.MILLISECONDS)
        .build()
    private val cameraExecutor = Executors.newSingleThreadExecutor()

    private val locationClient = LocationServices.getFusedLocationProviderClient(context)
    private var locationCallback: LocationCallback? = null
    private var latestLocation: LocationSnapshot? = null

    private var sessionId: String = UUID.randomUUID().toString()
    private var webSocket: WebSocket? = null
    private var audioJob: Job? = null
    private var running = false
    private var wsOpen = false
    private val lastFrameSentAtMs = AtomicLong(0L)

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

        connectLiveSocket(serverBaseUrl, wsToken, onStatus)
        startLocation(onStatus)
        startAudio(onStatus)
        startCamera(onStatus)

        onStatus("Live session started: $sessionId")
        return true
    }

    suspend fun stop(onStatus: (String) -> Unit) {
        if (!running) return
        running = false

        webSocket?.close(1000, "client_stop")
        webSocket = null
        wsOpen = false

        locationCallback?.let { callback ->
            locationClient.removeLocationUpdates(callback)
        }
        locationCallback = null
        latestLocation = null

        audioJob?.cancelAndJoin()
        audioJob = null

        runCatching {
            ProcessCameraProvider.getInstance(context).get().unbindAll()
        }.onFailure { error ->
            onStatus("Camera cleanup failed: ${error.message}")
        }
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
                onStatus("WS message: ${text.take(80)}")
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
                latestLocation = LocationSnapshot(
                    lat = latest.latitude,
                    lng = latest.longitude,
                    accuracyM = latest.accuracy,
                )
            }
        }
        locationCallback = callback
        locationClient.requestLocationUpdates(
            request,
            callback,
            context.mainLooper,
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
                    val bytes = ByteArray(buffer.remaining())
                    buffer.get(bytes)

                    val kinematic = RustCoreBridge.analyzeDefaultWalkingFrame()
                    val depth = RustCoreBridge.detectDropAheadRgba(
                        rgbaBytes = bytes,
                        width = image.width,
                        height = image.height,
                        riskScore = kinematic.riskScore,
                        carryModeCode = 1,
                        gyroMagnitude = 0.0f,
                        nowMs = SystemClock.elapsedRealtime(),
                    )
                    sendMultimodalFrame(kinematic.riskScore, depth.sourceCode)
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

    private fun sendMultimodalFrame(riskScore: Float, depthSourceCode: Int) {
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
        val sensorHealth = JSONObject()
            .put("score", if (depthSourceCode == 1) 0.95 else 0.75)
            .put("flags", emptyList<String>())
            .put("degraded", false)
            .put("source", "android-native-rust-v1")

        val payload = JSONObject()
            .put("type", "multimodal_frame")
            .put("session_id", sessionId)
            .put("timestamp", Instant.now().toString())
            .put("frame_jpeg_base64", JSONObject.NULL)
            .put("motion_state", "walking_fast")
            .put("pitch", 0.0)
            .put("velocity", 1.4)
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

        webSocket?.send(payload.toString())
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
