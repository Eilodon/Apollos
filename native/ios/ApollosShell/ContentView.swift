import SwiftUI

struct ContentView: View {
    @State private var result: IOSKinematicResult?

    var body: some View {
        VStack(spacing: 16) {
            Text("Apollos Native Shell")
                .font(.headline)
            Text("ABI: \(RustCoreBridge.abiVersionHex())")

            Button("Run Rust FFI") {
                result = RustCoreBridge.analyzeDefaultWalkingFrame()
            }

            if let result {
                Text("Risk score: \(result.riskScore, specifier: "%.2f")")
                Text("Should capture: \(result.shouldCapture ? "yes" : "no")")
                Text("Yaw delta: \(result.yawDeltaDeg, specifier: "%.2f")")
            }
        }
        .padding(24)
    }
}

#Preview {
    ContentView()
}
