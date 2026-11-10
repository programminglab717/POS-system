package dev.keel.android

import android.app.Application

/** The app's process: it holds the kernel, which outlives any screen. */
class KeelApplication : Application() {
    /** The kernel, opened at its first call. */
    val host: KernelHost by lazy { KernelHost(this) }
}
