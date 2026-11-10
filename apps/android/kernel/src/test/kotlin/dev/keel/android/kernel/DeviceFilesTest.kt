package dev.keel.android.kernel

import java.io.File
import java.nio.file.Files
import kotlin.test.Test
import kotlin.test.assertContentEquals
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertNotNull
import kotlin.test.assertNull
import kotlin.test.assertTrue

/**
 * A device's files against the kernel built for the host (ADR-0027): drawn the first time and the
 * same after; or imported from a provisioned kit, which is then removed; secrets kept only wrapped.
 */
class DeviceFilesTest {
    /** A vault that wraps by reversing and marking: plainly not the secret, and checkable. */
    private object Mirror : Vault {
        override fun wrap(secret: ByteArray) = byteArrayOf(0x57) + secret.reversedArray()

        override fun unwrap(wrapped: ByteArray): ByteArray {
            check(wrapped.first() == 0x57.toByte()) { "not wrapped" }
            return wrapped.copyOfRange(1, wrapped.size).reversedArray()
        }
    }

    private fun temp(): File = Files.createTempDirectory("keel-device-files").toFile()

    @Test
    fun a_device_is_drawn_once_and_its_keys_kept_wrapped() {
        val dir = temp()
        try {
            val first = DeviceFiles.load(dir, Mirror)
            assertNull(first.sync, "a device drawn alone joins no network")
            assertEquals(32, first.signingKey.size)
            assertFalse(File(dir, "signing.key").readBytes().contentEquals(first.signingKey), "kept wrapped")
            val again = DeviceFiles.load(dir, Mirror)
            assertEquals(first.device, again.device)
            assertContentEquals(first.storeKey, again.storeKey)
        } finally {
            dir.deleteRecursively()
        }
    }

    @Test
    fun a_provisioned_kit_is_imported_once_and_removed() {
        val (dir, kit) = temp() to temp()
        try {
            val files = mapOf(
                "device" to "0192f0c1-0000-7000-8000-0000000000d1\n".toByteArray(),
                "signing.key" to ByteArray(32) { 1 },
                "store.key" to ByteArray(32) { 2 },
                "tls.key" to byteArrayOf(3, 3, 3),
                "tls.crt" to byteArrayOf(4),
                "ca.crt" to byteArrayOf(5),
                "roster" to byteArrayOf(6),
                "profile" to byteArrayOf(7),
            )
            for ((name, bytes) in files) File(kit, name).writeBytes(bytes)
            val imported = DeviceFiles.load(dir, Mirror, kit)
            assertEquals("0192f0c1-0000-7000-8000-0000000000d1", imported.device)
            assertContentEquals(ByteArray(32) { 1 }, imported.signingKey)
            assertContentEquals(byteArrayOf(7), imported.profile)
            val sync = assertNotNull(imported.sync)
            assertContentEquals(byteArrayOf(3, 3, 3), sync.tlsKey)
            assertContentEquals(byteArrayOf(6), sync.roster)
            assertFalse(kit.exists(), "the kit's plaintext secrets are gone")
            for (secret in listOf("signing.key", "store.key", "tls.key")) {
                assertEquals(0x57.toByte(), File(dir, secret).readBytes().first(), "$secret kept wrapped")
            }
            // A kit that comes later changes nothing, and is removed: a device is made once, and
            // the kit's secrets are in the clear. Found in review: it stayed.
            kit.mkdirs()
            for ((name, bytes) in files) File(kit, name).writeBytes(if (name == "device") "other".toByteArray() else bytes)
            assertEquals(imported.device, DeviceFiles.load(dir, Mirror, kit).device)
            assertFalse(kit.exists(), "a later kit is removed too")
            assertTrue(dir.listFiles()!!.none { it.name.endsWith(".partial") }, "no partial files left")
        } finally {
            dir.deleteRecursively()
            kit.deleteRecursively()
        }
    }

    @Test
    fun a_kit_missing_a_file_isnt_taken_and_is_removed() {
        val (dir, kit) = temp() to temp()
        try {
            File(kit, "device").writeText("0192f0c1-0000-7000-8000-0000000000d1")
            val drawn = DeviceFiles.load(dir, Mirror, kit)
            assertNull(drawn.sync, "an incomplete kit isn't taken")
            assertFalse(kit.exists(), "but it is removed")
        } finally {
            dir.deleteRecursively()
            kit.deleteRecursively()
        }
    }
}
