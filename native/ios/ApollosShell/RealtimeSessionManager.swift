import AVFoundation
import Combine
import CoreLocation
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
    private var latestLocation: IOSLocationSnapshot?

    private var captureSession: AVCaptureSession?
    private let cameraQueue = DispatchQueue(label: "com.apollos.ios.camera")
    private var lastFrameSentAt: TimeInterval = 0

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

                DispatchQueue.main.async {
                    self.isRunning = true
                }

                connectWebSocket(serverBaseURL: trimmedURL, wsToken: wsToken)
                startLocation()
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
            return
        }

        isRunning = false

        wsTask?.cancel(with: .normalClosure, reason: nil)
        wsTask = nil

        locationManager.stopUpdatingLocation()
        latestLocation = nil

        if let session = captureSession {
            session.stopRunning()
        }
        captureSession = nil

        if let engine = audioEngine {
            engine.inputNode.removeTap(onBus: 0)
            engine.stop()
        }
        audioEngine = nil

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
                    self.appendLog("WS message: \(String(text.prefix(120)))")
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

    private func startLocation() {
        locationManager.desiredAccuracy = kCLLocationAccuracyBest
        locationManager.distanceFilter = 1
        locationManager.startUpdatingLocation()
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

    private func sendMultimodalFrame(riskScore: Float, depthSourceCode: UInt8) {
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

        let payload: [String: Any?] = [
            "type": "multimodal_frame",
            "session_id": sessionId,
            "timestamp": iso8601Now(),
            "frame_jpeg_base64": nil,
            "motion_state": "walking_fast",
            "pitch": 0.0,
            "velocity": 1.4,
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
                "score": depthSourceCode == 1 ? 0.95 : 0.75,
                "flags": [],
                "degraded": false,
                "source": "ios-native-rust-v1",
            ],
        ]

        sendJSON(payload)
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
        latestLocation = IOSLocationSnapshot(
            lat: latest.coordinate.latitude,
            lng: latest.coordinate.longitude,
            accuracyM: latest.horizontalAccuracy,
            capturedAt: Date()
        )
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

        let src = baseAddress.assumingMemoryBound(to: UInt8.self)
        var rgba = [UInt8](repeating: 0, count: width * height * 4)

        for y in 0..<height {
            let srcRow = src.advanced(by: y * bytesPerRow)
            for x in 0..<width {
                let srcOffset = x * 4
                let dstOffset = (y * width + x) * 4
                // Convert BGRA -> RGBA expected by Rust FFI.
                rgba[dstOffset] = srcRow[srcOffset + 2]
                rgba[dstOffset + 1] = srcRow[srcOffset + 1]
                rgba[dstOffset + 2] = srcRow[srcOffset]
                rgba[dstOffset + 3] = srcRow[srcOffset + 3]
            }
        }

        let kinematic = RustCoreBridge.analyzeDefaultWalkingFrame()
        let depth = RustCoreBridge.detectDropAheadRgba(
            rgba: rgba,
            width: UInt32(width),
            height: UInt32(height),
            riskScore: kinematic.riskScore,
            carryModeCode: 1,
            gyroMagnitude: 0,
            nowMs: UInt64(Date().timeIntervalSince1970 * 1000)
        )
        sendMultimodalFrame(riskScore: kinematic.riskScore, depthSourceCode: depth.sourceCode)
    }
}
