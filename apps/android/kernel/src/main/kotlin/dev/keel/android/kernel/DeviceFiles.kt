package dev.keel.android.kernel

import dev.keel.kernel.SyncKit
import dev.keel.kernel.demoProfile
import dev.keel.kernel.newDevice
import java.io.File
import java.io.FileOutputStream

/** What wraps the device's secrets: on Android, a key the Keystore holds. */
interface Vault {
    /** [secret], wrapped. */
    fun wrap(secret: ByteArray): ByteArray

    /** The secret [wrapped] holds. */
    fun unwrap(wrapped: ByteArray): ByteArray
}

/**
 * Who this device is, and what it joins its location with: its identifier, its keys unwrapped
 * for the kernel while it runs, the location's profile, and, for a provisioned device, the kit
 * it syncs with.
 */
class DeviceFiles(
    val device: String,
    val signingKey: ByteArray,
    val storeKey: ByteArray,
    val profile: ByteArray,
    /** The kit's roster and credentials, for a device provisioned to join its location. */
    val sync: SyncKit?,
) {
    companion object {
        /** The files of a provisioned kit (ADR-0026), as `keel-provision` writes them. */
        private val KIT = listOf("device", "signing.key", "store.key", "tls.key", "tls.crt", "ca.crt", "roster", "profile")

        /** The files whose contents are secret, kept only wrapped. */
        private val SECRETS = setOf("signing.key", "store.key", "tls.key")

        /**
         * The device kept in [dir]. The first time, it is made: from the provisioned kit in [kit],
         * if there is one; otherwise drawn by the kernel, for a register on the demo café's
         * profile, alone. Secrets are written only wrapped by [vault]; each file is written whole
         * and synced to the disk before the next, and the identifier last, so a device is made
         * only once all its files are. A kit found once the device is made, or left by an import
         * that stopped before removing it, is removed: its secrets are in the clear. Found in
         * review: it stayed, and the files weren't synced.
         *
         * A kit is pushed whole, into a directory beside `kit` and then moved there, so it is
         * never seen half-written.
         */
        fun load(dir: File, vault: Vault, kit: File? = null): DeviceFiles {
            val device = File(dir, "device")
            if (!device.exists()) {
                dir.mkdirs()
                if (kit != null && KIT.all { File(kit, it).isFile }) {
                    for (name in KIT - "device") {
                        val bytes = File(kit, name).readBytes()
                        write(File(dir, name), if (name in SECRETS) vault.wrap(bytes) else bytes)
                    }
                    write(device, File(kit, "device").readText().trim().toByteArray())
                } else {
                    val made = newDevice()
                    write(File(dir, "signing.key"), vault.wrap(made.signingKey))
                    write(File(dir, "store.key"), vault.wrap(made.storeKey))
                    write(File(dir, "profile"), demoProfile())
                    write(device, made.device.toByteArray())
                }
            }
            // The kit's secrets now live only wrapped, if at all.
            if (kit != null && kit.exists()) {
                check(kit.deleteRecursively()) { "the provisioned kit at $kit couldn't be removed" }
            }
            val read = { name: String -> File(dir, name).readBytes() }
            val sync = if (File(dir, "roster").isFile) {
                SyncKit(roster = read("roster"), ca = read("ca.crt"), certificate = read("tls.crt"), tlsKey = vault.unwrap(read("tls.key")))
            } else {
                null
            }
            return DeviceFiles(
                device = device.readText(),
                signingKey = vault.unwrap(read("signing.key")),
                storeKey = vault.unwrap(read("store.key")),
                profile = read("profile"),
                sync = sync,
            )
        }

        /** Writes [bytes] to [file] whole: to a file beside it, synced to the disk, then renamed. */
        private fun write(file: File, bytes: ByteArray) {
            val partial = File(file.parentFile, "${file.name}.partial")
            FileOutputStream(partial).use { out ->
                out.write(bytes)
                out.fd.sync()
            }
            check(partial.renameTo(file)) { "$file couldn't be written" }
        }
    }
}
