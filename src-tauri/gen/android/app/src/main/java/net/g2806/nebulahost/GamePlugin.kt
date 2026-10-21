package net.g2806.nebulahost

import android.app.Activity
import android.app.ActivityManager
import android.content.Context
import android.content.Intent
import android.os.Process
import app.tauri.annotation.Command
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.JSObject
import app.tauri.plugin.Plugin
import net.kdt.pojavlaunch.NebulaBootstrapActivity

/** Only the Rust backend calls these commands; the game Activity is private. */
@TauriPlugin
class GamePlugin(private val currentActivity: Activity) : Plugin(currentActivity) {
    private var pendingUntil = 0L
    private fun gameProcess(): ActivityManager.RunningAppProcessInfo? {
        val manager = currentActivity.getSystemService(Context.ACTIVITY_SERVICE) as ActivityManager
        return manager.runningAppProcesses?.firstOrNull { it.uid == Process.myUid() && it.processName == currentActivity.packageName + ":game" }
    }

    @Command
    fun start(invoke: Invoke) {
        if (gameProcess() != null || System.currentTimeMillis() < pendingUntil) {
            invoke.reject("A Minecraft game is already running")
            return
        }
        val request = invoke.getArgs().toString()
        pendingUntil = System.currentTimeMillis() + 15000
        currentActivity.runOnUiThread {
            try {
                val intent = Intent(currentActivity, NebulaBootstrapActivity::class.java)
                    .putExtra("nebulaRequest", request)
                    .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
                currentActivity.startActivity(intent)
                invoke.resolve(JSObject())
            } catch (error: Exception) {
                pendingUntil = 0
                invoke.reject("Cannot open Minecraft: " + error.javaClass.simpleName)
            }
        }
    }

    @Command
    fun status(invoke: Invoke) {
        val response = JSObject()
        response.put("running", gameProcess() != null || System.currentTimeMillis() < pendingUntil)
        invoke.resolve(response)
    }

    @Command
    fun stop(invoke: Invoke) {
        pendingUntil = 0
        gameProcess()?.let { Process.killProcess(it.pid) }
        invoke.resolve(JSObject())
    }
}
