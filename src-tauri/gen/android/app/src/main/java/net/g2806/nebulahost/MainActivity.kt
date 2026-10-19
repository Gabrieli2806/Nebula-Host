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
    super.onCreate(savedInstanceState)
  }
}
