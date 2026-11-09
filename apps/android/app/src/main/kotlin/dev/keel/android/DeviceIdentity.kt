package dev.keel.android

import dev.keel.kernel.newDevice
import java.io.File

/** Who this device is: its identifier, and its keys, unwrapped for the kernel while it runs. */
class DeviceIdentity(val device: String, val signingKey: ByteArray, val storeKey: ByteArray) {
    companion object {
        /**
         * The identity kept in [dir], made the first time: the kernel draws it, and the keys are
         * written only wrapped by [vault].
         */
        fun load(dir: File, vault: KeyVault): DeviceIdentity {
            val (device, signing, store) = listOf("device", "signing.key", "store.key").map { File(dir, it) }
            if (!device.exists()) {
                val made = newDevice()
                dir.mkdirs()
                // The keys first and the identifier last: a device is made once all three are.
                signing.writeBytes(vault.wrap(made.signingKey))
                store.writeBytes(vault.wrap(made.storeKey))
                device.writeText(made.device)
            }
            return DeviceIdentity(
                device.readText(),
                vault.unwrap(signing.readBytes()),
                vault.unwrap(store.readBytes()),
            )
        }
    }
}
