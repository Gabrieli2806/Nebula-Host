package net.g2806.nebulahost

import android.Manifest
import android.content.Intent
import android.content.pm.PackageManager
import android.os.Build
import android.os.Bundle
import android.system.Os
import androidx.activity.enableEdgeToEdge

class MainActivity : TauriActivity() {
  override fun onCreate(savedInstanceState: Bundle?) {
    enableEdgeToEdge()
    // The Rust side reads these when it starts, so they come before super:
    // where the launcher library is (the only place Android runs programs
    // from), the app's private folder, and the package itself, whose assets
    // hold the Java runtimes the person can install from the app.
    Os.setenv("NEBULA_NATIVE_LIB_DIR", applicationInfo.nativeLibraryDir, true)
    Os.setenv("NEBULA_FILES_DIR", filesDir.absolutePath, true)
    Os.setenv("NEBULA_APK_PATH", applicationInfo.sourceDir, true)
    // The phone's name ("Pixel 8") instead of a hostname, and the folders the
    // app may keep servers in without any storage permission: its private
    // folder and its own folder on each storage volume (phone, SD card).
    val deviceName = android.provider.Settings.Global.getString(contentResolver, "device_name")
      ?: "${Build.MANUFACTURER} ${Build.MODEL}"
    Os.setenv("NEBULA_DEVICE_NAME", deviceName, true)
    val volumes = getExternalFilesDirs(null).filterNotNull().joinToString("\n") { it.absolutePath }
    Os.setenv("NEBULA_STORAGE_DIRS", volumes, true)
    super.onCreate(savedInstanceState)
    captureShortcut(intent)

    // The notification is how servers keep running in the background.
    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU &&
      checkSelfPermission(Manifest.permission.POST_NOTIFICATIONS) != PackageManager.PERMISSION_GRANTED
    ) {
      requestPermissions(arrayOf(Manifest.permission.POST_NOTIFICATIONS), 1)
    }
  }

  override fun onNewIntent(intent: Intent) {
    super.onNewIntent(intent)
    captureShortcut(intent)
  }

  private fun captureShortcut(intent: Intent?) {
    val uri = intent?.data ?: return
    if (intent.action != "net.g2806.nebulahost.PLAY_INSTANCE" || uri.scheme != "nebula" || uri.host != "instance") return
    val id = uri.lastPathSegment ?: return
    if (!id.matches(Regex("[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}"))) return
    // Account/mode come from our private preferences, never external extras.
    val selection = getSharedPreferences("launcher-shortcuts", MODE_PRIVATE).getString(id, null) ?: return
    val request = org.json.JSONObject(selection).put("requestId", java.util.UUID.randomUUID().toString())
    synchronized(MainActivity::class.java) { pendingShortcut = request.toString() }
    intent.data = null
  }

  override fun onResume() {
    super.onResume()
    visible = true
    // Started with the app; it stays while servers run, after the app leaves.
    ServerService.start(this)
  }

  override fun onPause() {
    visible = false
    super.onPause()
  }

  companion object {
    private var pendingShortcut: String? = null
    fun takeShortcut(): String? = synchronized(MainActivity::class.java) {
      val value = pendingShortcut
      pendingShortcut = null
      value
    }
    @Volatile
    var visible = false
  }
}
