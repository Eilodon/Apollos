package com.apollos.nativeapp

import android.content.ComponentName
import android.content.Context
import android.content.Intent
import android.content.ServiceConnection
import android.os.Bundle
import android.os.IBinder
import android.os.SystemClock
import android.view.KeyEvent
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.lifecycle.lifecycleScope
import com.apollos.nativeapp.ui.ModernAppShell
import kotlinx.coroutines.launch

class MainActivity : ComponentActivity() {
    private var boundService: ApollosService? = null
    private var isBound = false

    private val connection = object : ServiceConnection {
        override fun onServiceConnected(name: ComponentName?, service: IBinder?) {
            val binder = service as? ApollosService.LocalBinder ?: return
            boundService = binder.getService()
        }

        override fun onServiceDisconnected(name: ComponentName?) {
            boundService = null
        }
    }

    private var lastVolUpClickTime = 0L
    private val DOUBLE_CLICK_TIME = 500L
    private var isVolDownLongPressed = false

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        setContent {
            ModernAppShell()
        }
    }

    override fun onStart() {
        super.onStart()
        Intent(this, ApollosService::class.java).also { intent ->
            bindService(intent, connection, Context.BIND_AUTO_CREATE)
            isBound = true
        }
    }

    override fun onStop() {
        super.onStop()
        if (isBound) {
            unbindService(connection)
            isBound = false
        }
    }

    override fun onKeyDown(keyCode: Int, event: KeyEvent): Boolean {
        when (keyCode) {
            KeyEvent.KEYCODE_VOLUME_UP -> {
                if (event.repeatCount == 0) {
                    val clickTime = SystemClock.elapsedRealtime()
                    if (clickTime - lastVolUpClickTime < DOUBLE_CLICK_TIME) {
                        handleSessionToggle()
                        lastVolUpClickTime = 0L
                    } else {
                        lastVolUpClickTime = clickTime
                    }
                }
                return true
            }
            KeyEvent.KEYCODE_VOLUME_DOWN -> return true
        }
        return super.onKeyDown(keyCode, event)
    }

    override fun onKeyLongPress(keyCode: Int, event: KeyEvent): Boolean {
        if (keyCode == KeyEvent.KEYCODE_VOLUME_DOWN) {
            if (!isVolDownLongPressed) {
                isVolDownLongPressed = true
                boundService?.sessionManager?.setMicActive(true)
            }
            return true
        }
        return super.onKeyLongPress(keyCode, event)
    }

    override fun onKeyUp(keyCode: Int, event: KeyEvent): Boolean {
        if (keyCode == KeyEvent.KEYCODE_VOLUME_DOWN) {
            if (isVolDownLongPressed) {
                isVolDownLongPressed = false
                boundService?.sessionManager?.setMicActive(false)
            }
            return true
        }
        return super.onKeyUp(keyCode, event)
    }

    private fun handleSessionToggle() {
        val manager = boundService?.sessionManager ?: return
        lifecycleScope.launch {
            if (manager.running) {
                manager.stop()
            } else {
                val serverBaseUrl = loadSavedServerBaseUrl(this@MainActivity) ?: BuildConfig.SERVER_URL
                val idToken = loadSavedIdToken(this@MainActivity).orEmpty()
                manager.start(serverBaseUrl, idToken) { }
            }
        }
    }
}
