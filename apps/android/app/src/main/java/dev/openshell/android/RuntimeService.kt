package dev.openshell.android

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.Context
import android.content.Intent
import android.content.pm.ServiceInfo
import android.net.ConnectivityManager
import android.net.Network
import android.net.NetworkCapabilities
import android.os.Build
import android.os.Handler
import android.os.IBinder
import android.os.Looper
import android.os.PowerManager
import org.json.JSONObject
import java.util.concurrent.Executors

/**
 * Foreground service that keeps the running app's services alive (type `specialUse`): the web
 * server, the database and functions keep serving the app and paired devices with the screen off.
 * Shows a notification with the running services; stops when the user taps "Stop".
 */
class RuntimeService : Service() {
    companion object {
        const val EXTRA_IDENTIFIER = "identifier"
        const val EXTRA_DIR = "dir"
        const val EXTRA_NAME = "name"
        const val ACTION_STOP = "dev.openshell.android.STOP"
        private const val CHANNEL = "runtime"
        private const val NOTIFICATION_ID = 1

        /** Last status of the runtime (`NativeRuntime.status()` / `start()`), for the activities. */
        @Volatile
        var status: JSONObject = JSONObject().put("running", false)
            private set
        private val listeners = mutableListOf<(JSONObject) -> Unit>()
        private val main = Handler(Looper.getMainLooper())
        private val worker = Executors.newSingleThreadExecutor()

        fun addListener(l: (JSONObject) -> Unit) = synchronized(listeners) { listeners.add(l) }
        fun removeListener(l: (JSONObject) -> Unit) = synchronized(listeners) { listeners.remove(l) }

        private fun publish(s: JSONObject) {
            status = s
            val ls = synchronized(listeners) { listeners.toList() }
            main.post { ls.forEach { it(s) } }
        }

        fun start(ctx: Context, identifier: String, dir: String, name: String) {
            val i = Intent(ctx, RuntimeService::class.java)
                .putExtra(EXTRA_IDENTIFIER, identifier)
                .putExtra(EXTRA_DIR, dir)
                .putExtra(EXTRA_NAME, name)
            ctx.startForegroundService(i)
        }

        fun stop(ctx: Context) {
            ctx.startService(Intent(ctx, RuntimeService::class.java).setAction(ACTION_STOP))
        }

        /** Forward a lifecycle event to the core (ignored when nothing runs). */
        fun lifecycle(event: JSONObject) {
            worker.execute { runCatching { NativeRuntime.lifecycle(event.toString()) } }
        }
    }

    private var current: String? = null
    private var appName = "OpenShell"
    private var wakeLock: PowerManager.WakeLock? = null
    private var network: ConnectivityManager.NetworkCallback? = null
    private val refresh = object : Runnable {
        override fun run() {
            worker.execute {
                val s = JSONObject(NativeRuntime.status())
                publish(s)
                main.post { notifyStatus(s) }
            }
            main.postDelayed(this, 15_000)
        }
    }

    override fun onBind(intent: Intent?): IBinder? = null

    override fun onCreate() {
        super.onCreate()
        val channel = NotificationChannel(CHANNEL, getString(R.string.channel_name), NotificationManager.IMPORTANCE_LOW)
        channel.description = getString(R.string.channel_description)
        getSystemService(NotificationManager::class.java).createNotificationChannel(channel)
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        if (intent?.action == ACTION_STOP) {
            shutdown()
            return START_NOT_STICKY
        }
        val id = intent?.getStringExtra(EXTRA_IDENTIFIER) ?: return START_NOT_STICKY
        val dir = intent.getStringExtra(EXTRA_DIR) ?: return START_NOT_STICKY
        appName = intent.getStringExtra(EXTRA_NAME) ?: id
        val notification = buildNotification(getString(R.string.notification_starting))
        if (Build.VERSION.SDK_INT >= 34) {
            startForeground(NOTIFICATION_ID, notification, ServiceInfo.FOREGROUND_SERVICE_TYPE_SPECIAL_USE)
        } else {
            startForeground(NOTIFICATION_ID, notification)
        }
        if (current == id && status.optBoolean("running")) {
            publish(status)
            return START_NOT_STICKY
        }
        current = id
        acquireWakeLock()
        watchNetwork()
        val state = Apps.stateDir(this, id).absolutePath
        val libs = applicationInfo.nativeLibraryDir
        worker.execute {
            val s = JSONObject(NativeRuntime.start(dir, state, libs, "production"))
            publish(s)
            main.post { notifyStatus(s) }
        }
        main.removeCallbacks(refresh)
        main.postDelayed(refresh, 15_000)
        return START_NOT_STICKY
    }

    private fun notifyStatus(s: JSONObject) {
        val text = if (!s.optBoolean("ok", true)) {
            s.optString("error", "failed to start")
        } else {
            val running = mutableListOf<String>()
            val services = s.optJSONArray("services")
            if (services != null) {
                for (i in 0 until services.length()) {
                    val svc = services.getJSONObject(i)
                    if (svc.optString("state") in setOf("running", "starting", "setup", "building")) {
                        running.add(svc.optString("name"))
                    }
                }
            }
            if (running.isEmpty()) "No services running" else running.joinToString(", ")
        }
        getSystemService(NotificationManager::class.java).notify(NOTIFICATION_ID, buildNotification(text))
    }

    private fun buildNotification(text: String): Notification {
        val open = PendingIntent.getActivity(
            this, 0, Intent(this, LauncherActivity::class.java), PendingIntent.FLAG_IMMUTABLE
        )
        val stop = PendingIntent.getService(
            this, 1, Intent(this, RuntimeService::class.java).setAction(ACTION_STOP), PendingIntent.FLAG_IMMUTABLE
        )
        return Notification.Builder(this, CHANNEL)
            .setSmallIcon(R.drawable.ic_notification)
            .setContentTitle(getString(R.string.notification_title, appName))
            .setContentText(text)
            .setContentIntent(open)
            .setOngoing(true)
            .addAction(Notification.Action.Builder(null, getString(R.string.action_stop), stop).build())
            .build()
    }

    private fun acquireWakeLock() {
        if (wakeLock?.isHeld == true) return
        // partial wake lock: the CPU keeps serving requests with the screen off
        val lock = getSystemService(PowerManager::class.java)
            .newWakeLock(PowerManager.PARTIAL_WAKE_LOCK, "openshell:runtime")
        lock.setReferenceCounted(false)
        lock.acquire()
        wakeLock = lock
    }

    private fun watchNetwork() {
        if (network != null) return
        val cb = object : ConnectivityManager.NetworkCallback() {
            override fun onCapabilitiesChanged(n: Network, caps: NetworkCapabilities) {
                val metered = !caps.hasCapability(NetworkCapabilities.NET_CAPABILITY_NOT_METERED)
                lifecycle(JSONObject().put("kind", "network").put("connected", true).put("metered", metered))
            }

            override fun onLost(n: Network) {
                lifecycle(JSONObject().put("kind", "network").put("connected", false).put("metered", false))
            }
        }
        getSystemService(ConnectivityManager::class.java).registerDefaultNetworkCallback(cb)
        network = cb
    }

    private fun releaseResources() {
        main.removeCallbacks(refresh)
        network?.let { getSystemService(ConnectivityManager::class.java).unregisterNetworkCallback(it) }
        network = null
        wakeLock?.let { if (it.isHeld) it.release() }
        wakeLock = null
    }

    private fun shutdown() {
        releaseResources()
        worker.execute {
            NativeRuntime.stop()
            publish(JSONObject().put("running", false))
        }
        current = null
        stopForeground(STOP_FOREGROUND_REMOVE)
        stopSelf()
    }

    override fun onDestroy() {
        releaseResources()
        super.onDestroy()
    }
}
