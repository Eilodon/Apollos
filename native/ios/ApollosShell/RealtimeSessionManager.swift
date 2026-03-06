import AVFoundation
import AudioToolbox
import Combine
import CoreLocation
import CoreMotion
import Foundation

struct NativePermissionStatus {
    var camera: Bool
    var microphone: Bool
    var location: Bool
}

struct IOSLocationSnapshot {
    let lat: Double
    let lng: Double
    let accuracyM: Double
    let capturedAt: Date
}

private struct OidcExchangeResponse: Decodable {
    let session_token: String
}

private struct WsTicketResponse: Decodable {
    let access_token: String
}

final class RealtimeSessionManager: NSObject, ObservableObject {
    private let safetyDeadzoneScore: Float = 0.1

    @Published private(set) var isRunning: Bool = false
    @Published private(set) var logs: [String] = []
    @Published private(set) var permissions = NativePermissionStatus(
        camera: false,
        microphone: false,
        location: false
    )

    private var sessionId = UUID().uuidString
    private var wsTask: URLSessionWebSocketTask?
    private let urlSession = URLSession(configuration: .default)
    private let locationManager = CLLocationManager()
    private let motionManager = CMMotionManager()
    private let motionQueue: OperationQueue = {
        let queue = OperationQueue()
        queue.name = "com.apollos.ios.motion"
        return queue
    }()
    private let imuLock = NSLock()
    private var latestLocation: IOSLocationSnapshot?
    private var geoOrigin: IOSLocationSnapshot?
    private var eskfHandle: UInt64 = 0
    private var lastMotionTimestamp: TimeInterval = 0
    private var latestGyroMagnitudeDeg: Float = 0

    private var captureSession: AVCaptureSession?
    private let cameraQueue = DispatchQueue(label: "com.apollos.ios.camera")
    private var lastFrameSentAt: TimeInterval = 0
    private var lastCameraFrameTimestampS: TimeInterval = 0
    private var latestVoApplied: Bool = false
    private var latestVoFlowScore: Float = 0.0
    private var latestVoVarianceM2: Float = 999.0
    private var latestVoPoseXM: Float = 0.0
    private var latestVoPoseYM: Float = 0.0
    private let safetyActuatorLock = NSLock()
    private var safetyToneEngine: AVAudioEngine?
    private var safetyToneNode: AVAudioPlayerNode?
    private var latestSafetyDirectiveAt: TimeInterval = 0

    private var audioEngine: AVAudioEngine?

    override init() {
        super.init()
        locationManager.delegate = self
        refreshPermissions()
    }

    deinit {
        stop()
    }

    func refreshPermissions() {
        DispatchQueue.main.async {
            self.permissions = NativePermissionStatus(
                camera: AVCaptureDevice.authorizationStatus(for: .video) == .authorized,
                microphone: AVAudioSession.sharedInstance().recordPermission == .granted,
                location: CLLocationManager.authorizationStatus() == .authorizedWhenInUse
                    || CLLocationManager.authorizationStatus() == .authorizedAlways
            )
        }
    }

    func requestPermissions() {
        AVCaptureDevice.requestAccess(for: .video) { [weak self] _ in
            self?.refreshPermissions()
        }
        AVAudioSession.sharedInstance().requestRecordPermission { [weak self] _ in
            self?.refreshPermissions()
        }
        locationManager.requestWhenInUseAuthorization()
    }

    func start(serverBaseURL: String, idToken: String) {
        if isRunning {
            return
        }

        let trimmedURL = serverBaseURL.trimmingCharacters(in: .whitespacesAndNewlines)
        let trimmedToken = idToken.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmedURL.isEmpty, !trimmedToken.isEmpty else {
            appendLog("Server URL and OIDC token are required")
            return
        }

        let hasAllPermissions =
            permissions.camera && permissions.microphone && permissions.location
        if !hasAllPermissions {
            appendLog("Missing permissions (camera/mic/location)")
            return
        }

        sessionId = UUID().uuidString

        Task {
            do {
                let wsToken = try await fetchAuthTokens(
                    serverBaseURL: trimmedURL,
                    idToken: trimmedToken
                )
                let handle = RustCoreBridge.eskfCreate()
                guard handle != 0 else {
                    appendLog("ESKF create failed")
                    return
                }

                DispatchQueue.main.async {
                    self.isRunning = true
                }
                eskfHandle = handle
                geoOrigin = nil
                lastMotionTimestamp = 0
                setGyroMagnitudeDeg(0)
                lastCameraFrameTimestampS = 0
                latestVoApplied = false
                latestVoFlowScore = 0.0
                latestVoVarianceM2 = 999.0
                latestVoPoseXM = 0.0
                latestVoPoseYM = 0.0
                latestSafetyDirectiveAt = 0

                connectWebSocket(serverBaseURL: trimmedURL, wsToken: wsToken)
                startLocation()
                startMotion()
                startCamera()
                startAudio()

                appendLog("Live session started: \(sessionId)")
            } catch {
                appendLog("Failed to start: \(error.localizedDescription)")
            }
        }
    }

    func stop() {
        if !isRunning {
            if eskfHandle != 0 {
                _ = RustCoreBridge.eskfDestroy(handle: eskfHandle)
                eskfHandle = 0
            }
            return
        }

        isRunning = false

        wsTask?.cancel(with: .normalClosure, reason: nil)
        wsTask = nil

        locationManager.stopUpdatingLocation()
        latestLocation = nil
        geoOrigin = nil

        motionManager.stopDeviceMotionUpdates()
        lastMotionTimestamp = 0
        setGyroMagnitudeDeg(0)
        lastCameraFrameTimestampS = 0
        latestVoApplied = false
        latestVoFlowScore = 0.0
        latestVoVarianceM2 = 999.0
        latestVoPoseXM = 0.0
        latestVoPoseYM = 0.0
        latestSafetyDirectiveAt = 0
        if eskfHandle != 0 {
            _ = RustCoreBridge.eskfDestroy(handle: eskfHandle)
            eskfHandle = 0
        }

        if let session = captureSession {
            session.stopRunning()
        }
        captureSession = nil

        if let engine = audioEngine {
            engine.inputNode.removeTap(onBus: 0)
            engine.stop()
        }
        audioEngine = nil
        stopSafetyActuator()

        appendLog("Stopped")
    }

    private func fetchAuthTokens(serverBaseURL: String, idToken: String) async throws -> String {
        let exchangeURL = try endpointURL(
            baseURL: serverBaseURL,
            path: "/auth/oidc/exchange"
        )
        let exchangePayload: [String: Any] = ["id_token": idToken]
        let exchange: OidcExchangeResponse = try await postJSON(
            url: exchangeURL,
            payload: exchangePayload
        )

        let wsURL = try endpointURL(baseURL: serverBaseURL, path: "/auth/ws-ticket")
        let wsPayload: [String: Any] = ["session_token": exchange.session_token]
        let ws: WsTicketResponse = try await postJSON(url: wsURL, payload: wsPayload)
        return ws.access_token
    }

    private func connectWebSocket(serverBaseURL: String, wsToken: String) {
        do {
            let url = try liveWebSocketURL(baseURL: serverBaseURL, sessionId: sessionId)
            let tokenB64 = base64UrlEncode(raw: wsToken)
            let task = urlSession.webSocketTask(
                with: url,
                protocols: ["authb64.\(tokenB64)", "apollos.v1"]
            )
            wsTask = task
            task.resume()
            appendLog("WS connected")
            receiveLoop()
        } catch {
            appendLog("WS connect failed: \(error.localizedDescription)")
        }
    }

    private func receiveLoop() {
        guard let wsTask else {
            return
        }

        wsTask.receive { [weak self] result in
            guard let self else {
                return
            }

            switch result {
            case .success(let message):
                switch message {
                case .string(let text):
                    self.handleServerMessage(text)
                case .data(let data):
                    self.appendLog("WS binary message: \(data.count) bytes")
                @unknown default:
                    self.appendLog("WS unknown message")
                }
                if self.isRunning {
                    self.receiveLoop()
                }
            case .failure(let error):
                self.appendLog("WS receive failed: \(error.localizedDescription)")
                self.stop()
            }
        }
    }

    private func handleServerMessage(_ text: String) {
        guard
            let data = text.data(using: .utf8),
            let payload = try? JSONSerialization.jsonObject(with: data) as? [String: Any]
        else {
            appendLog("WS non-json message: \(String(text.prefix(80)))")
            return
        }

        let type = (payload["type"] as? String) ?? ""
        switch type {
        case "safety_directive":
            applySafetyDirective(payload)
        case "connection_state":
            let detail = (payload["detail"] as? String) ?? "state_update"
            appendLog("WS state: \(detail)")
        case "cognition_state":
            let layer = (payload["active_layer"] as? String) ?? "unknown_layer"
            let reason = (payload["reason"] as? String) ?? "no_reason"
            appendLog("WS cognition: \(layer) (\(reason))")
        default:
            appendLog("WS message: \(String(text.prefix(120)))")
        }
    }

    private func applySafetyDirective(_ payload: [String: Any]) {
        let hazardScore = Float(payload["hazard_score"] as? Double ?? 0.0)
        let hardStop = payload["hard_stop"] as? Bool ?? false
        let hapticIntensity = max(
            0.0,
            min(1.0, Float(payload["haptic_intensity"] as? Double ?? 0.0))
        )
        let pitchHz = Float(payload["spatial_audio_pitch_hz"] as? Double ?? 0.0)

        let now = Date().timeIntervalSince1970
        if (now - latestSafetyDirectiveAt) < 0.06 {
            return
        }
        latestSafetyDirectiveAt = now

        if hazardScore < safetyDeadzoneScore || hapticIntensity <= 0 || pitchHz <= 0 {
            stopSafetyActuator()
            return
        }

        let durationMs = hardStop ? 220 : 90
        let gain = max(0.08, min(1.0, hapticIntensity))
        playSafetyTone(frequencyHz: pitchHz, gain: gain, durationMs: durationMs)
        triggerSafetyHaptic()
    }

    private func ensureSafetyToneEngine() -> (AVAudioEngine, AVAudioPlayerNode)? {
        safetyActuatorLock.lock()
        defer { safetyActuatorLock.unlock() }

        if let existingEngine = safetyToneEngine, let existingNode = safetyToneNode {
            return (existingEngine, existingNode)
        }

        do {
            let session = AVAudioSession.sharedInstance()
            try session.setCategory(
                .playAndRecord,
                mode: .voiceChat,
                options: [.defaultToSpeaker, .allowBluetooth]
            )
            try session.setActive(true)
        } catch {
            appendLog("Safety audio session failed: \(error.localizedDescription)")
            return nil
        }

        let engine = AVAudioEngine()
        let node = AVAudioPlayerNode()
        guard let format = AVAudioFormat(standardFormatWithSampleRate: 16_000, channels: 1) else {
            appendLog("Safety audio format unavailable")
            return nil
        }

        engine.attach(node)
        engine.connect(node, to: engine.mainMixerNode, format: format)
        do {
            try engine.start()
        } catch {
            appendLog("Safety audio engine failed: \(error.localizedDescription)")
            return nil
        }

        safetyToneEngine = engine
        safetyToneNode = node
        return (engine, node)
    }

    private func playSafetyTone(frequencyHz: Float, gain: Float, durationMs: Int) {
        guard let (_, node) = ensureSafetyToneEngine() else {
            return
        }
        guard let format = AVAudioFormat(standardFormatWithSampleRate: 16_000, channels: 1) else {
            return
        }

        let sampleCount = max(1, (16_000 * durationMs) / 1000)
        guard
            let buffer = AVAudioPCMBuffer(
                pcmFormat: format,
                frameCapacity: AVAudioFrameCount(sampleCount)
            ),
            let channelData = buffer.floatChannelData?[0]
        else {
            return
        }

        buffer.frameLength = AVAudioFrameCount(sampleCount)
        let step = (2.0 * Float.pi * frequencyHz) / 16_000.0
        var phase: Float = 0.0
        for idx in 0..<sampleCount {
            let envelope = 1.0 - (Float(idx) / Float(sampleCount))
            channelData[idx] = sinf(phase) * gain * envelope
            phase += step
        }

        safetyActuatorLock.lock()
        node.stop()
        node.scheduleBuffer(buffer, at: nil, options: [])
        node.play()
        safetyActuatorLock.unlock()
    }

    private func triggerSafetyHaptic() {
        AudioServicesPlaySystemSound(kSystemSoundID_Vibrate)
    }

    private func stopSafetyActuator() {
        safetyActuatorLock.lock()
        safetyToneNode?.stop()
        safetyToneEngine?.stop()
        safetyToneNode = nil
        safetyToneEngine = nil
        safetyActuatorLock.unlock()
    }

    private func startLocation() {
        locationManager.desiredAccuracy = kCLLocationAccuracyBest
        locationManager.distanceFilter = 1
        locationManager.startUpdatingLocation()
    }

    private func startMotion() {
        guard motionManager.isDeviceMotionAvailable else {
            appendLog("CoreMotion unavailable")
            return
        }

        motionManager.deviceMotionUpdateInterval = 1.0 / 100.0
        motionManager.startDeviceMotionUpdates(to: motionQueue) { [weak self] motion, _ in
            guard let self, self.isRunning, let motion else {
                return
            }

            let ts = motion.timestamp
            if self.lastMotionTimestamp <= 0 {
                self.lastMotionTimestamp = ts
                return
            }

            let dtS = Float(max(0.0, min(0.2, ts - self.lastMotionTimestamp)))
            self.lastMotionTimestamp = ts
            if dtS <= 0 {
                return
            }

            let g = Float(9.80665)
            let accelX = Float(motion.userAcceleration.x) * g
            let accelY = Float(motion.userAcceleration.y) * g
            let accelZ = Float(motion.userAcceleration.z) * g
            _ = RustCoreBridge.eskfPredictImu(
                handle: self.eskfHandle,
                accelX: accelX,
                accelY: accelY,
                accelZ: accelZ,
                dtS: dtS
            )

            let rate = motion.rotationRate
            let gyroRad = sqrt(rate.x * rate.x + rate.y * rate.y + rate.z * rate.z)
            let gyroDeg = Float(gyroRad * 57.29578)
            self.setGyroMagnitudeDeg(gyroDeg)
        }
    }

    private func setGyroMagnitudeDeg(_ value: Float) {
        imuLock.lock()
        latestGyroMagnitudeDeg = value
        imuLock.unlock()
    }

    private func currentGyroMagnitudeDeg() -> Float {
        imuLock.lock()
        let value = latestGyroMagnitudeDeg
        imuLock.unlock()
        return value
    }

    private func ingestLocationToEskf(_ location: IOSLocationSnapshot) {
        let origin: IOSLocationSnapshot
        if let existing = geoOrigin {
            origin = existing
        } else {
            geoOrigin = location
            origin = location
        }

        let latScaleM = 111_132.0
        let lngScaleM = max(1.0, 111_320.0 * cos(origin.lat * .pi / 180.0))
        let posX = Float((location.lng - origin.lng) * lngScaleM)
        let posY = Float((location.lat - origin.lat) * latScaleM)
        let varianceM2 = Float(max(4.0, min(50.0, location.accuracyM)))
        _ = RustCoreBridge.eskfUpdateVision(
            handle: eskfHandle,
            positionX: posX,
            positionY: posY,
            positionZ: 0.0,
            varianceM2: varianceM2 * varianceM2
        )
    }

    private func startCamera() {
        guard AVCaptureDevice.authorizationStatus(for: .video) == .authorized else {
            appendLog("Camera permission missing")
            return
        }

        let session = AVCaptureSession()
        session.beginConfiguration()
        session.sessionPreset = .vga640x480

        guard
            let camera = AVCaptureDevice.default(.builtInWideAngleCamera, for: .video, position: .back),
            let input = try? AVCaptureDeviceInput(device: camera),
            session.canAddInput(input)
        else {
            appendLog("Camera unavailable")
            return
        }
        session.addInput(input)

        let output = AVCaptureVideoDataOutput()
        output.videoSettings = [
            kCVPixelBufferPixelFormatTypeKey as String: kCVPixelFormatType_32BGRA
        ]
        output.alwaysDiscardsLateVideoFrames = true
        output.setSampleBufferDelegate(self, queue: cameraQueue)

        guard session.canAddOutput(output) else {
            appendLog("Camera output unavailable")
            return
        }
        session.addOutput(output)
        session.commitConfiguration()

        captureSession = session
        cameraQueue.async {
            session.startRunning()
        }
    }

    private func startAudio() {
        guard AVAudioSession.sharedInstance().recordPermission == .granted else {
            appendLog("Mic permission missing")
            return
        }

        do {
            let audioSession = AVAudioSession.sharedInstance()
            try audioSession.setCategory(
                .playAndRecord,
                mode: .voiceChat,
                options: [.defaultToSpeaker, .allowBluetooth]
            )
            try audioSession.setPreferredSampleRate(16_000)
            try audioSession.setActive(true)

            let engine = AVAudioEngine()
            let inputNode = engine.inputNode
            let format = inputNode.outputFormat(forBus: 0)

            inputNode.installTap(onBus: 0, bufferSize: 1024, format: format) { [weak self] buffer, _ in
                guard let self else {
                    return
                }
                if !self.isRunning {
                    return
                }

                guard let pcmData = self.pcm16Data(from: buffer) else {
                    return
                }
                self.sendAudioChunk(pcmData)
            }

            try engine.start()
            audioEngine = engine
        } catch {
            appendLog("Audio start failed: \(error.localizedDescription)")
        }
    }

    private func sendMultimodalFrame(riskScore: Float, depth: IOSDepthHazardResult) {
        guard isRunning else {
            return
        }

        let now = Date().timeIntervalSince1970
        if (now - lastFrameSentAt) < 0.2 {
            return
        }
        lastFrameSentAt = now

        let location = latestLocation
        let nowDate = Date()
        let locationAgeMs = location.map {
            Int(nowDate.timeIntervalSince($0.capturedAt) * 1000)
        } ?? 0
        let eskf = RustCoreBridge.eskfSnapshot(handle: eskfHandle)
        var healthFlags: [String] = []
        if eskf.degraded {
            healthFlags.append("eskf_degraded")
        }
        if eskf.localizationUncertaintyM > 6.0 {
            healthFlags.append("localization_uncertain")
        }
        if depth.sourceCode != 1 {
            healthFlags.append("depth_heuristic_fallback")
        }
        if !latestVoApplied {
            healthFlags.append("vision_odometry_fallback")
        }
        let velocityMps = max(0.2, min(3.0, riskScore))
        let visionOdometry: [String: Any] = [
            "source": latestVoApplied ? "ios-visual-odometry-v1" : "gps-anchor-fallback",
            "applied": latestVoApplied,
            "optical_flow_score": max(0.0, min(1.0, latestVoFlowScore)),
            "variance_m2": max(0.0, min(999.0, latestVoVarianceM2)),
            "pose_x_m": latestVoPoseXM,
            "pose_y_m": latestVoPoseYM,
        ]
        let cloudLink: [String: Any] = [
            "connected": wsTask != nil,
            "rtt_ms": NSNull(),
            "source": "ios-live-ws-v1",
        ]
        let edgeSemanticCues = buildEdgeSemanticCues(depth: depth)

        let payload: [String: Any?] = [
            "type": "multimodal_frame",
            "session_id": sessionId,
            "timestamp": iso8601Now(),
            "frame_jpeg_base64": nil,
            "motion_state": "walking_fast",
            "pitch": 0.0,
            "velocity": velocityMps,
            "user_text": nil,
            "yaw_delta_deg": 0.0,
            "carry_mode": "necklace",
            "sensor_unavailable": false,
            "lat": location?.lat,
            "lng": location?.lng,
            "heading_deg": nil,
            "location_accuracy_m": location?.accuracyM,
            "location_age_ms": locationAgeMs,
            "sensor_health": [
                "score": max(0.0, min(1.0, eskf.sensorHealthScore)),
                "flags": healthFlags,
                "degraded": eskf.degraded,
                "source": "ios-eskf-runtime-v3",
            ],
            "sensor_uncertainty": [
                "covariance_3x3": [
                    eskf.covarianceXx, 0.0, 0.0,
                    0.0, eskf.covarianceYy, 0.0,
                    0.0, 0.0, eskf.covarianceZz,
                ],
                "innovation_norm": max(0.0, min(10.0, eskf.innovationNorm)),
                "source": "ios-eskf-runtime-v3",
            ],
            "vision_odometry": visionOdometry,
            "cloud_link": cloudLink,
            "edge_semantic_cues": edgeSemanticCues,
        ]

        sendJSON(payload)
    }

    private func buildEdgeSemanticCues(depth: IOSDepthHazardResult) -> [[String: Any]] {
        guard depth.detected else {
            return []
        }

        let distanceM: Float
        switch depth.distanceCode {
        case 0:
            distanceM = 1.0
        case 1:
            distanceM = 2.5
        case 2:
            distanceM = 4.5
        default:
            distanceM = 3.0
        }

        return [[
            "cue_type": "drop_ahead",
            "text": "Drop ahead",
            "confidence": max(0.0, min(1.0, depth.confidence)),
            "position_x": max(-1.0, min(1.0, depth.positionX)),
            "distance_m": distanceM,
            "position_clock": clockFaceFromPositionX(depth.positionX),
            "ttl_ms": 1200,
            "source": depth.sourceCode == 1 ? "edge_depth_onnx" : "edge_depth_heuristic",
        ]]
    }

    private func clockFaceFromPositionX(_ positionX: Float) -> String {
        if positionX <= -0.6 { return "10h" }
        if positionX <= -0.25 { return "11h" }
        if positionX < 0.25 { return "12h" }
        if positionX < 0.6 { return "1h" }
        return "2h"
    }

    private func sendAudioChunk(_ pcm16: Data) {
        let payload: [String: Any] = [
            "type": "audio_chunk",
            "session_id": sessionId,
            "timestamp": iso8601Now(),
            "audio_chunk_pcm16": pcm16.base64EncodedString(),
        ]
        sendJSON(payload)
    }

    private func sendHazardObservation(depth: IOSDepthHazardResult, riskScore: Float) {
        let distanceM: Float
        switch depth.distanceCode {
        case 0:
            distanceM = 1.0
        case 1:
            distanceM = 2.5
        case 2:
            distanceM = 4.5
        default:
            distanceM = 3.0
        }
        let relativeVelocityMps = -max(0.4, min(3.0, riskScore))
        let source = depth.sourceCode == 1 ? "depth_onnx" : "depth_heuristic"

        let payload: [String: Any] = [
            "type": "hazard_observation",
            "session_id": sessionId,
            "timestamp": iso8601Now(),
            "hazard_type": "DROP_AHEAD",
            "bearing_x": depth.positionX,
            "distance_m": distanceM,
            "relative_velocity_mps": relativeVelocityMps,
            "confidence": depth.confidence,
            "source": source,
            "suppress_ms": 3000,
        ]
        sendJSON(payload)
    }

    private func sendJSON(_ payload: [String: Any?]) {
        guard let wsTask else {
            return
        }

        let compact = payload.reduce(into: [String: Any]()) { partial, entry in
            if let value = entry.value {
                partial[entry.key] = value
            } else {
                partial[entry.key] = NSNull()
            }
        }

        guard
            let data = try? JSONSerialization.data(withJSONObject: compact, options: []),
            let text = String(data: data, encoding: .utf8)
        else {
            return
        }

        wsTask.send(.string(text)) { [weak self] error in
            if let error {
                self?.appendLog("WS send failed: \(error.localizedDescription)")
            }
        }
    }

    private func appendLog(_ message: String) {
        DispatchQueue.main.async {
            self.logs.append(message)
            if self.logs.count > 24 {
                self.logs.removeFirst(self.logs.count - 24)
            }
        }
    }

    private func postJSON<T: Decodable>(
        url: URL,
        payload: [String: Any]
    ) async throws -> T {
        var request = URLRequest(url: url)
        request.httpMethod = "POST"
        request.setValue("application/json", forHTTPHeaderField: "Content-Type")
        request.httpBody = try JSONSerialization.data(withJSONObject: payload, options: [])

        let (data, response) = try await urlSession.data(for: request)
        guard let http = response as? HTTPURLResponse else {
            throw URLError(.badServerResponse)
        }
        guard (200..<300).contains(http.statusCode) else {
            let body = String(data: data, encoding: .utf8) ?? ""
            throw NSError(
                domain: "ApollosHTTPError",
                code: http.statusCode,
                userInfo: [NSLocalizedDescriptionKey: "HTTP \(http.statusCode): \(body)"]
            )
        }

        return try JSONDecoder().decode(T.self, from: data)
    }

    private func endpointURL(baseURL: String, path: String) throws -> URL {
        guard var components = URLComponents(string: baseURL) else {
            throw URLError(.badURL)
        }
        components.path = path
        components.query = nil
        guard let url = components.url else {
            throw URLError(.badURL)
        }
        return url
    }

    private func liveWebSocketURL(baseURL: String, sessionId: String) throws -> URL {
        guard var components = URLComponents(string: baseURL) else {
            throw URLError(.badURL)
        }

        if components.scheme == "https" {
            components.scheme = "wss"
        } else {
            components.scheme = "ws"
        }
        components.path = "/ws/live/\(sessionId)"
        components.query = nil

        guard let url = components.url else {
            throw URLError(.badURL)
        }
        return url
    }

    private func base64UrlEncode(raw: String) -> String {
        Data(raw.utf8)
            .base64EncodedString()
            .replacingOccurrences(of: "+", with: "-")
            .replacingOccurrences(of: "/", with: "_")
            .replacingOccurrences(of: "=", with: "")
    }

    private func iso8601Now() -> String {
        ISO8601DateFormatter().string(from: Date())
    }

    private func pcm16Data(from buffer: AVAudioPCMBuffer) -> Data? {
        guard let floatData = buffer.floatChannelData else {
            return nil
        }
        let frameLength = Int(buffer.frameLength)
        if frameLength <= 0 {
            return nil
        }

        var pcm = Data(capacity: frameLength * MemoryLayout<Int16>.size)
        for index in 0..<frameLength {
            let sample = floatData[0][index]
            let clamped = max(-1.0, min(1.0, sample))
            var value = Int16(clamped * Float(Int16.max))
            pcm.append(Data(bytes: &value, count: MemoryLayout<Int16>.size))
        }
        return pcm
    }
}

extension RealtimeSessionManager: CLLocationManagerDelegate {
    func locationManagerDidChangeAuthorization(_ manager: CLLocationManager) {
        refreshPermissions()
    }

    func locationManager(_ manager: CLLocationManager, didUpdateLocations locations: [CLLocation]) {
        guard let latest = locations.last else {
            return
        }
        let snapshot = IOSLocationSnapshot(
            lat: latest.coordinate.latitude,
            lng: latest.coordinate.longitude,
            accuracyM: latest.horizontalAccuracy,
            capturedAt: Date()
        )
        latestLocation = snapshot
        ingestLocationToEskf(snapshot)
    }
}

extension RealtimeSessionManager: AVCaptureVideoDataOutputSampleBufferDelegate {
    func captureOutput(
        _ output: AVCaptureOutput,
        didOutput sampleBuffer: CMSampleBuffer,
        from connection: AVCaptureConnection
    ) {
        guard isRunning else {
            return
        }

        guard let pixelBuffer = CMSampleBufferGetImageBuffer(sampleBuffer) else {
            return
        }

        CVPixelBufferLockBaseAddress(pixelBuffer, .readOnly)
        defer {
            CVPixelBufferUnlockBaseAddress(pixelBuffer, .readOnly)
        }

        let width = CVPixelBufferGetWidth(pixelBuffer)
        let height = CVPixelBufferGetHeight(pixelBuffer)
        let bytesPerRow = CVPixelBufferGetBytesPerRow(pixelBuffer)
        guard let baseAddress = CVPixelBufferGetBaseAddress(pixelBuffer) else {
            return
        }
        let bufferLen = bytesPerRow * height

        let frameTimestamp = CMSampleBufferGetPresentationTimeStamp(sampleBuffer).seconds
        let currentTimestamp = frameTimestamp.isFinite ? frameTimestamp : Date().timeIntervalSince1970
        let dtS: Float
        if lastCameraFrameTimestampS <= 0 {
            dtS = 0.033
        } else {
            dtS = Float(max(0.01, min(0.2, currentTimestamp - lastCameraFrameTimestampS)))
        }
        lastCameraFrameTimestampS = currentTimestamp

        let vo = RustCoreBridge.eskfUpdateVisualOdometryBgraStrided(
            handle: eskfHandle,
            baseAddress: baseAddress,
            bufferLen: bufferLen,
            width: UInt32(width),
            height: UInt32(height),
            rowStride: UInt32(bytesPerRow),
            pixelStride: 4,
            dtS: dtS
        )
        latestVoApplied = vo.applied
        latestVoFlowScore = vo.opticalFlowScore
        latestVoVarianceM2 = vo.varianceM2
        latestVoPoseXM = vo.poseXM
        latestVoPoseYM = vo.poseYM

        let kinematic = RustCoreBridge.analyzeDefaultWalkingFrame()
        let depth = RustCoreBridge.detectDropAheadBgraStrided(
            baseAddress: baseAddress,
            bufferLen: bufferLen,
            width: UInt32(width),
            height: UInt32(height),
            rowStride: UInt32(bytesPerRow),
            pixelStride: 4,
            riskScore: kinematic.riskScore,
            carryModeCode: 1,
            gyroMagnitude: currentGyroMagnitudeDeg(),
            nowMs: UInt64(Date().timeIntervalSince1970 * 1000)
        )
        if depth.detected {
            sendHazardObservation(depth: depth, riskScore: kinematic.riskScore)
        }
        sendMultimodalFrame(riskScore: kinematic.riskScore, depth: depth)
    }
}
