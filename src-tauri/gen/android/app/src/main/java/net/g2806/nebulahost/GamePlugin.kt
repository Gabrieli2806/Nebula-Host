package net.g2806.nebulahost

import android.app.Activity
import android.app.ActivityManager
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.content.pm.ShortcutInfo
import android.content.pm.ShortcutManager
import android.graphics.drawable.Icon
import android.net.Uri
import android.os.Build
import android.os.Process
import app.tauri.annotation.Command
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.JSArray
import app.tauri.plugin.JSObject
import app.tauri.plugin.Plugin
import java.io.File
import net.g2806.nebula.game.BootstrapActivity

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
    private var startedAt = 0L
    private val activities get() = currentActivity.getSystemService(Context.ACTIVITY_SERVICE) as ActivityManager

    private fun gameProcess(): ActivityManager.RunningAppProcessInfo? =
        activities.runningAppProcesses?.firstOrNull { it.uid == Process.myUid() && it.processName == currentActivity.packageName + ":game" }

    /** Every task the game made: its window lives in its own task
     *  (taskAffinity …game), which Recents keeps even after the process died. */
    private fun gameTasks(): List<ActivityManager.AppTask> =
        activities.appTasks.filter { it.taskInfo.baseIntent.component?.className?.startsWith(GAME_PACKAGE) == true }

    /** The task of a game that is really running: the process is alive and one
     *  of the game's screens is on top of the task. A task alone is only a
     *  Recents entry; bringing it back would recreate a screen without a game. */
    private fun liveGameTask(): ActivityManager.AppTask? {
        if (gameProcess() == null) return null
        return gameTasks().firstOrNull { it.taskInfo.topActivity?.className?.startsWith(GAME_PACKAGE) == true }
    }

    /** End a game process that has no game in it (a failed or abandoned
     *  start) and clear stale game entries from Recents, so a new start never
     *  lands in a process that already ran a JVM. */
    private fun clearStaleGame() {
        gameProcess()?.let { stale ->
            Process.killProcess(stale.pid)
            val deadline = System.currentTimeMillis() + 3000
            while (gameProcess() != null && System.currentTimeMillis() < deadline) Thread.sleep(50)
        }
        gameTasks().forEach { task -> runCatching { task.finishAndRemoveTask() } }
    }

    @Command
    fun start(invoke: Invoke) {
        if (System.currentTimeMillis() < pendingUntil) {
            invoke.reject("A Minecraft game is already starting")
            return
        }
        // Already playing: Play (or a home screen shortcut) returns to it.
        liveGameTask()?.let { task ->
            currentActivity.runOnUiThread { task.moveToFront() }
            invoke.resolve(JSObject().also { it.put("resumed", true) })
            return
        }
        val request = invoke.getArgs().toString()
        startedAt = System.currentTimeMillis()
        pendingUntil = startedAt + 15000
        Thread({
            try {
                clearStaleGame()
            } catch (error: Exception) {
                android.util.Log.w("NebulaGame", "Cannot clear a stale game", error)
            }
            currentActivity.runOnUiThread {
                try {
                    // A fresh task every time: never stacked on an old game window.
                    val intent = Intent(currentActivity, BootstrapActivity::class.java)
                        .putExtra("nebulaRequest", request)
                        .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_CLEAR_TASK)
                    currentActivity.startActivity(intent)
                    invoke.resolve(JSObject())
                } catch (error: Exception) {
                    pendingUntil = 0
                    invoke.reject("Cannot open Minecraft: " + error.javaClass.simpleName)
                }
            }
        }, "nebula-game-start").start()
    }

    @Command
    fun status(invoke: Invoke) {
        val now = System.currentTimeMillis()
        val live = liveGameTask() != null
        // Once the game's own screen is up the start is over; from then on
        // only a live game counts, so a failed start shows within a second.
        if (live && gameTasks().any { it.taskInfo.topActivity?.className == GAME_ACTIVITY }) pendingUntil = 0
        val starting = now < pendingUntil && (gameProcess() != null || now - startedAt < 3000)
        invoke.resolve(JSObject().also { it.put("running", live || starting) })
    }

    /** Bring Nebula back from the browser once Microsoft sign-in is done. */
    @Command
    fun focusApp(invoke: Invoke) {
        val intent = Intent(currentActivity, MainActivity::class.java)
            .addFlags(Intent.FLAG_ACTIVITY_REORDER_TO_FRONT or Intent.FLAG_ACTIVITY_NEW_TASK)
        currentActivity.runOnUiThread { currentActivity.startActivity(intent) }
        invoke.resolve(JSObject())
    }

    /** Bring the running game's window back to the front. */
    @Command
    fun resume(invoke: Invoke) {
        val task = liveGameTask()
        if (task == null) {
            invoke.reject("No Minecraft game is running")
            return
        }
        currentActivity.runOnUiThread { task.moveToFront() }
        invoke.resolve(JSObject())
    }

    /** What this phone can render with, for the instance's graphics settings:
     *  Vulkan (the Zink renderer and newer Minecraft's own Vulkan backend),
     *  the OpenGL ES version, and which optional renderers this APK carries. */
    @Command
    fun deviceGraphics(invoke: Invoke) {
        val pm = currentActivity.packageManager
        val vulkanLevel = if (Build.VERSION.SDK_INT >= 24) pm.systemAvailableFeatures.firstOrNull { it.name == PackageManager.FEATURE_VULKAN_HARDWARE_VERSION }?.version ?: 0 else 0
        val hasVulkan = Build.VERSION.SDK_INT >= 24 &&
            pm.hasSystemFeature(PackageManager.FEATURE_VULKAN_HARDWARE_LEVEL) && vulkanLevel != 0
        val manager = currentActivity.getSystemService(Context.ACTIVITY_SERVICE) as ActivityManager
        val gles = manager.deviceConfigurationInfo.reqGlEsVersion
        val libs = File(currentActivity.applicationInfo.nativeLibraryDir)
        val renderers = JSArray()
        renderers.put("mobileglues")
        if (gles >= 0x30000 && File(libs, "libng_gl4es.so").exists()) renderers.put("krypton")
        if (hasVulkan) renderers.put("zink")
        if (gles >= 0x30000 && File(libs, "libltw.so").exists()) renderers.put("ltw")
        val memory = ActivityManager.MemoryInfo().also { manager.getMemoryInfo(it) }
        val response = JSObject()
        response.put("vulkan", hasVulkan)
        // Encoded as major << 22 | minor << 12 | patch, like VK_MAKE_VERSION.
        response.put("vulkan_version", if (hasVulkan) "${vulkanLevel shr 22}.${(vulkanLevel shr 12) and 0x3ff}" else null)
        response.put("gles_version", "${gles shr 16}.${gles and 0xffff}")
        response.put("renderers", renderers)
        response.put("total_memory_mb", memory.totalMem / (1024 * 1024))
        response.put("gpu", if (Build.VERSION.SDK_INT >= 31) Build.SOC_MODEL else Build.HARDWARE)
        invoke.resolve(response)
    }

    @Command
    fun stop(invoke: Invoke) {
        pendingUntil = 0
        gameProcess()?.let { Process.killProcess(it.pid) }
        gameTasks().forEach { task -> runCatching { task.finishAndRemoveTask() } }
        invoke.resolve(JSObject())
    }

    private companion object {
        const val GAME_PACKAGE = "net.g2806.nebula.game."
        const val GAME_ACTIVITY = "net.g2806.nebula.game.GameActivity"
    }
}
