package net.g2806.nebulahost

import android.app.Activity
import android.app.ActivityManager
import android.content.Context
import android.content.Intent
import android.content.pm.ShortcutInfo
import android.content.pm.ShortcutManager
import android.graphics.drawable.Icon
import android.net.Uri
import android.os.Build
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
    @Command
    fun pinShortcut(invoke: Invoke) {
        if (Build.VERSION.SDK_INT < 26) { invoke.reject("Home screen shortcuts require Android 8 or newer"); return }
        val manager = currentActivity.getSystemService(ShortcutManager::class.java)
        if (!manager.isRequestPinShortcutSupported) { invoke.reject("Your home screen does not support pinned shortcuts"); return }
        val args = invoke.getArgs()
        val id = args.optString("id")
        val mode = args.optString("mode")
        if (!id.matches(Regex("[0-9a-f-]{36}")) || (mode != "demo" && mode != "account")) {
            invoke.reject("Invalid shortcut"); return
        }
        val label = args.optString("name").take(64)
        val intent = Intent(currentActivity, MainActivity::class.java)
            .setAction("net.g2806.nebulahost.PLAY_INSTANCE")
            .setData(Uri.parse("nebula://instance/$id"))
            .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_CLEAR_TOP or Intent.FLAG_ACTIVITY_SINGLE_TOP)
        currentActivity.getSharedPreferences("launcher-shortcuts", Context.MODE_PRIVATE).edit().putString(id, args.toString()).apply()
        val shortcut = ShortcutInfo.Builder(currentActivity, "minecraft-$id")
            .setShortLabel(label).setLongLabel(label)
            .setIcon(Icon.createWithResource(currentActivity, R.mipmap.ic_launcher))
            .setIntent(intent).build()
        currentActivity.runOnUiThread {
            try {
                if (!manager.requestPinShortcut(shortcut, null)) invoke.reject("Home screen did not accept the shortcut request")
                else invoke.resolve()
            } catch (error: Exception) { invoke.reject("Cannot request shortcut: " + error.javaClass.simpleName) }
        }
    }

    @Command
    fun takeShortcut(invoke: Invoke) {
        val value = MainActivity.takeShortcut()
        if (value == null) invoke.resolve() else invoke.resolve(JSObject(value))
    }

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

    /** Bring the running game's window back to the front. */
    @Command
    fun resume(invoke: Invoke) {
        val manager = currentActivity.getSystemService(Context.ACTIVITY_SERVICE) as ActivityManager
        val task = manager.appTasks.firstOrNull { it.taskInfo.baseActivity?.className?.startsWith("net.kdt.pojavlaunch.") == true }
        if (task == null) {
            invoke.reject("No Minecraft game is running")
            return
        }
        task.moveToFront()
        invoke.resolve(JSObject())
    }

    @Command
    fun stop(invoke: Invoke) {
        pendingUntil = 0
        gameProcess()?.let { Process.killProcess(it.pid) }
        invoke.resolve(JSObject())
    }
}
