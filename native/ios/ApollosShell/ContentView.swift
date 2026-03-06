import SwiftUI

struct ContentView: View {
    @StateObject private var realtime = RealtimeSessionManager()
    @State private var serverBaseURL: String = "http://127.0.0.1:8000"
    @State private var idToken: String = ""
    @State private var result: IOSKinematicResult?

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 12) {
                Text("Apollos Native Shell")
                    .font(.headline)
                Text("ABI: \(RustCoreBridge.abiVersionHex())")
                Text("Depth ONNX runtime: \(RustCoreBridge.depthOnnxEnabled() ? "on" : "off")")

                TextField("Server base URL", text: $serverBaseURL)
                    .textFieldStyle(.roundedBorder)
                    .textInputAutocapitalization(.never)
                    .autocorrectionDisabled()

                TextField("OIDC ID token", text: $idToken, axis: .vertical)
                    .textFieldStyle(.roundedBorder)
                    .textInputAutocapitalization(.never)
                    .autocorrectionDisabled()
                    .lineLimit(4)

                HStack(spacing: 8) {
                    Button("Grant Permissions") {
                        realtime.requestPermissions()
                    }
                    Button("Start Live") {
                        realtime.start(serverBaseURL: serverBaseURL, idToken: idToken)
                    }
                    .disabled(realtime.isRunning)

                    Button("Stop") {
                        realtime.stop()
                    }
                    .disabled(!realtime.isRunning)
                }

                Button("Run Rust FFI") {
                    result = RustCoreBridge.analyzeDefaultWalkingFrame()
                }

                if let result {
                    Text("Risk score: \(result.riskScore, specifier: "%.2f")")
                    Text("Should capture: \(result.shouldCapture ? "yes" : "no")")
                    Text("Yaw delta: \(result.yawDeltaDeg, specifier: "%.2f")")
                }

                Text("Permissions")
                    .font(.subheadline.weight(.semibold))
                Text("Camera: \(realtime.permissions.camera ? "granted" : "missing")")
                Text("Mic: \(realtime.permissions.microphone ? "granted" : "missing")")
                Text("Location: \(realtime.permissions.location ? "granted" : "missing")")

                Text("Session Log")
                    .font(.subheadline.weight(.semibold))
                if realtime.logs.isEmpty {
                    Text("No events yet")
                        .foregroundStyle(.secondary)
                } else {
                    ForEach(Array(realtime.logs.enumerated()), id: \.offset) { _, log in
                        Text(log)
                            .font(.caption)
                            .frame(maxWidth: .infinity, alignment: .leading)
                    }
                }
            }
            .padding(24)
        }
        .task {
            realtime.refreshPermissions()
        }
        .onDisappear {
            realtime.stop()
        }
    }
}

#Preview {
    ContentView()
}
