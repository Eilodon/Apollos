package com.apollos.nativeapp

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.Button
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp

@Composable
fun MainScreen() {
    val result = remember { mutableStateOf<KinematicResult?>(null) }

    Column(
        modifier = Modifier
            .fillMaxSize()
            .padding(24.dp),
        verticalArrangement = Arrangement.Center,
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        Text(text = "Apollos Native Shell", style = MaterialTheme.typography.headlineSmall)
        Text(text = "ABI: 0x${RustCoreBridge.abiVersion().toString(16)}")

        Button(onClick = { result.value = RustCoreBridge.analyzeDefaultWalkingFrame() }) {
            Text(text = "Run Rust FFI")
        }

        result.value?.let { output ->
            Text(text = "Risk score: ${output.riskScore}")
            Text(text = "Should capture: ${output.shouldCapture}")
            Text(text = "Yaw delta: ${output.yawDeltaDeg}")
        }
    }
}
