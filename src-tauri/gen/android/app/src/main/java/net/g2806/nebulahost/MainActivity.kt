package net.g2806.nebulahost

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
      ?: "${android.os.Build.MANUFACTURER} ${android.os.Build.MODEL}"
    Os.setenv("NEBULA_DEVICE_NAME", deviceName, true)
    val volumes = getExternalFilesDirs(null).filterNotNull().joinToString("\n") { it.absolutePath }
    Os.setenv("NEBULA_STORAGE_DIRS", volumes, true)
    super.onCreate(savedInstanceState)
  }
}
