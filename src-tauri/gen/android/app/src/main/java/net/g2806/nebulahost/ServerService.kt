package net.g2806.nebulahost

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.Context
import android.content.Intent
import android.content.pm.ServiceInfo
import android.net.wifi.WifiManager
import android.os.Build
import android.os.Handler
import android.os.IBinder
import android.os.Looper
import android.os.PowerManager
import org.json.JSONObject
import java.io.File

/**
 * Keeps Nebula Host alive while servers run with the app in the background.
 *
 * Android stops a backgrounded app after a while; a foreground service with
 * a notification is how an app says "the user is relying on this right now".
 * The notification lists the running servers. While one runs, the CPU and
 * Wi-Fi are kept awake so players stay connected with the screen off.
 *
 * The Rust side writes `running.json` every few seconds (service/poller.rs);
 * this reads it. When nothing runs and the app is not on screen, it stops.
 */
class ServerService : Service() {
  private val handler = Handler(Looper.getMainLooper())
  private var wakeLock: PowerManager.WakeLock? = null
  private var wifiLock: WifiManager.WifiLock? = null

  private val tick = object : Runnable {
    override fun run() {
      refresh()
      handler.postDelayed(this, 5000)
    }
  }

  override fun onBind(intent: Intent?): IBinder? = null

  override fun onCreate() {
    super.onCreate()
    val manager = getSystemService(NotificationManager::class.java)
    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
      val channel = NotificationChannel(CHANNEL, getString(R.string.servers_channel), NotificationManager.IMPORTANCE_LOW)
      channel.setShowBadge(false)
      manager.createNotificationChannel(channel)
    }
    val notification = build(emptyList())
    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.UPSIDE_DOWN_CAKE) {
      startForeground(ID, notification, ServiceInfo.FOREGROUND_SERVICE_TYPE_SPECIAL_USE)
    } else {
      startForeground(ID, notification)
    }
    handler.post(tick)
  }

  override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int = START_STICKY

  override fun onDestroy() {
    handler.removeCallbacks(tick)
    release()
    super.onDestroy()
  }

  private fun refresh() {
    val running = readRunning()
    if (running.isEmpty() && !MainActivity.visible) {
      stopSelf()
      return
    }
    if (running.isEmpty()) release() else {
      hold()
      askAboutBattery()
    }
    getSystemService(NotificationManager::class.java).notify(ID, build(running))
  }

  private fun readRunning(): List<String> = try {
    val file = File(filesDir, "nebula-host/running.json")
    val list = JSONObject(file.readText()).getJSONArray("running")
    (0 until list.length()).map { index ->
      val server = list.getJSONObject(index)
      val players = server.optInt("players", 0)
      if (players > 0) getString(R.string.server_with_players, server.getString("name"), players)
      else server.getString("name")
    }
  } catch (_: Exception) {
    emptyList()
  }

  private fun build(running: List<String>): Notification {
    val open = PendingIntent.getActivity(
      this, 0,
      Intent(this, MainActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_SINGLE_TOP),
      PendingIntent.FLAG_IMMUTABLE,
    )
    val title = if (running.isEmpty()) getString(R.string.no_servers_running)
      else resources.getQuantityString(R.plurals.servers_running, running.size, running.size)
    val builder = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) Notification.Builder(this, CHANNEL)
      else @Suppress("DEPRECATION") Notification.Builder(this)
    return builder
      .setSmallIcon(R.drawable.ic_stat_nebula)
      .setContentTitle(title)
      .setContentText(running.joinToString(" · "))
      .setStyle(Notification.BigTextStyle().bigText(running.joinToString("\n")))
      .setContentIntent(open)
      .setOngoing(true)
      .setShowWhen(false)
      .build()
  }

  /**
   * Some phones pause background apps despite the notification (aggressive
   * battery savers). Once per app start, while a server runs and the app is
   * still battery-optimised, a notification offers the system setting that
   * lets it keep running. It opens the settings list rather than asking
   * directly: the direct prompt needs a permission Google Play reserves for
   * a few kinds of apps, and the foreground service already covers most
   * phones.
   */
  private fun askAboutBattery() {
    if (askedAboutBattery) return
    val power = getSystemService(Context.POWER_SERVICE) as PowerManager
    if (power.isIgnoringBatteryOptimizations(packageName)) return
    askedAboutBattery = true
    val settings = PendingIntent.getActivity(
      this, 1,
      Intent(android.provider.Settings.ACTION_IGNORE_BATTERY_OPTIMIZATION_SETTINGS).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK),
      PendingIntent.FLAG_IMMUTABLE,
    )
    val builder = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) Notification.Builder(this, CHANNEL)
      else @Suppress("DEPRECATION") Notification.Builder(this)
    val notification = builder
      .setSmallIcon(R.drawable.ic_stat_nebula)
      .setContentTitle(getString(R.string.battery_title))
      .setContentText(getString(R.string.battery_text))
      .setStyle(Notification.BigTextStyle().bigText(getString(R.string.battery_text)))
      .setContentIntent(settings)
      .setAutoCancel(true)
      .build()
    getSystemService(NotificationManager::class.java).notify(BATTERY_ID, notification)
  }

  private fun hold() {
    if (wakeLock == null) {
      wakeLock = (getSystemService(Context.POWER_SERVICE) as PowerManager)
        .newWakeLock(PowerManager.PARTIAL_WAKE_LOCK, "NebulaHost:servers").apply { acquire() }
    }
    if (wifiLock == null) {
      @Suppress("DEPRECATION")
      wifiLock = (applicationContext.getSystemService(Context.WIFI_SERVICE) as WifiManager)
        .createWifiLock(WifiManager.WIFI_MODE_FULL_HIGH_PERF, "NebulaHost:servers").apply { acquire() }
    }
  }

  private fun release() {
    wakeLock?.let { if (it.isHeld) it.release() }
    wakeLock = null
    wifiLock?.let { if (it.isHeld) it.release() }
    wifiLock = null
  }

  companion object {
    private const val CHANNEL = "servers"
    private const val ID = 47400
    private const val BATTERY_ID = 47401
    @Volatile private var askedAboutBattery = false

    fun start(context: Context) {
      val intent = Intent(context, ServerService::class.java)
      if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) context.startForegroundService(intent)
      else context.startService(intent)
    }
  }
}
