package com.apollos.nativeapp

import android.Manifest
import android.content.Context
import android.content.pm.PackageManager
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Button
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateListOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalLifecycleOwner
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import androidx.core.content.ContextCompat
import kotlinx.coroutines.launch

private val REQUIRED_PERMISSIONS = arrayOf(
    Manifest.permission.CAMERA,
    Manifest.permission.RECORD_AUDIO,
    Manifest.permission.ACCESS_FINE_LOCATION,
    Manifest.permission.ACCESS_COARSE_LOCATION,
)

@Composable
fun MainScreen() {
    val context = LocalContext.current
    val lifecycleOwner = LocalLifecycleOwner.current
    val scope = rememberCoroutineScope()
    val manager = remember(lifecycleOwner) {
        RealtimeSessionManager(
            context = context.applicationContext,
            lifecycleOwner = lifecycleOwner,
        )
    }
    val permissionStatuses = remember {
        mutableStateListOf(*REQUIRED_PERMISSIONS.map { permission ->
            permission to hasPermission(context, permission)
        }.toTypedArray())
    }
    var running by remember { mutableStateOf(false) }
    var serverBaseUrl by remember { mutableStateOf("http://10.0.2.2:8000") }
    var idToken by remember { mutableStateOf("") }
    val logs = remember { mutableStateListOf<String>() }
    val kinematicResult = remember { mutableStateOf<KinematicResult?>(null) }
    val depthOnnxEnabled = remember { RustCoreBridge.depthOnnxEnabled() }

    val permissionLauncher = rememberLauncherForActivityResult(
        contract = ActivityResultContracts.RequestMultiplePermissions(),
    ) { grants ->
        permissionStatuses.clear()
        permissionStatuses.addAll(REQUIRED_PERMISSIONS.map { permission ->
            permission to (grants[permission] ?: hasPermission(context, permission))
        })
        logs.add(
            if (allPermissionsGranted(context)) {
                "Permissions granted"
            } else {
                "Some permissions still missing"
            },
        )
    }

    fun appendStatus(value: String) {
        logs.add(value)
        if (
            value.startsWith("Auth failed") ||
                value.startsWith("WS failure") ||
                value.startsWith("WS closed") ||
                value == "Stopped"
        ) {
            running = false
        }
        while (logs.size > 20) {
            logs.removeAt(0)
        }
    }

    DisposableEffect(Unit) {
        onDispose {
            scope.launch {
                manager.stop(::appendStatus)
            }
        }
    }

    Column(
        modifier = Modifier
            .fillMaxSize()
            .verticalScroll(rememberScrollState())
            .padding(16.dp),
        verticalArrangement = Arrangement.spacedBy(12.dp),
        horizontalAlignment = Alignment.Start,
    ) {
        Text(text = "Apollos Native Shell", style = MaterialTheme.typography.headlineSmall)
        Text(text = "ABI: 0x${RustCoreBridge.abiVersion().toString(16)}")
        Text(text = "Depth ONNX runtime: ${if (depthOnnxEnabled) "on" else "off"}")

        OutlinedTextField(
            value = serverBaseUrl,
            onValueChange = { serverBaseUrl = it.trim() },
            modifier = Modifier.fillMaxWidth(),
            label = { Text("Server base URL") },
            singleLine = true,
        )

        OutlinedTextField(
            value = idToken,
            onValueChange = { idToken = it },
            modifier = Modifier.fillMaxWidth(),
            label = { Text("OIDC ID token") },
        )

        Row(
            modifier = Modifier.fillMaxWidth(),
            horizontalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            Button(
                onClick = {
                    permissionLauncher.launch(REQUIRED_PERMISSIONS)
                },
            ) {
                Text(text = "Grant Permissions")
            }

            Button(
                onClick = {
                    scope.launch {
                        if (!allPermissionsGranted(context)) {
                            appendStatus("Permissions missing; grant camera/mic/location first")
                            return@launch
                        }
                        if (idToken.trim().isEmpty()) {
                            appendStatus("OIDC ID token is required")
                            return@launch
                        }
                        running = manager.start(serverBaseUrl, idToken, ::appendStatus)
                    }
                },
                enabled = !running,
            ) {
                Text(text = "Start Live")
            }

            Button(
                onClick = {
                    scope.launch {
                        manager.stop(::appendStatus)
                        running = false
                    }
                },
                enabled = running,
            ) {
                Text(text = "Stop")
            }
        }

        Button(onClick = { kinematicResult.value = RustCoreBridge.analyzeDefaultWalkingFrame() }) {
            Text(text = "Run Rust FFI")
        }

        kinematicResult.value?.let { output ->
            Text(text = "Risk score: ${output.riskScore}")
            Text(text = "Should capture: ${output.shouldCapture}")
            Text(text = "Yaw delta: ${output.yawDeltaDeg}")
        }

        Spacer(modifier = Modifier.height(8.dp))
        Text(
            text = "Permissions",
            style = MaterialTheme.typography.titleMedium,
        )
        permissionStatuses.forEach { (permission, granted) ->
            Text(
                text = "$permission: ${if (granted) "granted" else "missing"}",
                style = MaterialTheme.typography.bodySmall,
            )
        }

        Spacer(modifier = Modifier.height(8.dp))
        Text(
            text = "Session Log",
            style = MaterialTheme.typography.titleMedium,
        )
        if (logs.isEmpty()) {
            Text(
                text = "No events yet",
                style = MaterialTheme.typography.bodySmall,
            )
        } else {
            logs.forEach { line ->
                Text(
                    text = line,
                    style = MaterialTheme.typography.bodySmall,
                )
            }
        }
    }
}

private fun hasPermission(context: Context, permission: String): Boolean {
    return ContextCompat.checkSelfPermission(context, permission) == PackageManager.PERMISSION_GRANTED
}

private fun allPermissionsGranted(context: Context): Boolean {
    return REQUIRED_PERMISSIONS.all { hasPermission(context, it) }
}
