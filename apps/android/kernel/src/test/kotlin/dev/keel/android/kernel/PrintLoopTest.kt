package dev.keel.android.kernel

import dev.keel.kernel.JobKind
import dev.keel.kernel.JobState
import dev.keel.kernel.Mode
import dev.keel.kernel.Register
import dev.keel.kernel.RegisterConfig
import dev.keel.kernel.demoProfile
import dev.keel.kernel.newDevice
import dev.keel.register.PrinterAddress
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.asCoroutineDispatcher
import kotlinx.coroutines.cancelAndJoin
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.withContext
import kotlinx.coroutines.withTimeout
import java.io.ByteArrayOutputStream
import java.net.ServerSocket
import java.nio.file.Files
import java.util.concurrent.Executors
import kotlin.concurrent.thread
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertTrue
import kotlin.time.Duration.Companion.milliseconds

/**
 * The print loop against the kernel built for the host (ADR-0027): a sale's drawer kick and
 * receipt reach a fake printer, each once, in order; and with no printer set, jobs wait.
 */
class PrintLoopTest {
    private fun id(n: Int) = "0192f0c1-0000-7000-8000-%012x".format(n)

    /** A register with a paid sale on it, in a directory of its own, closed and removed after. */
    private fun withSale(body: suspend CoroutineScope.(Register) -> Unit) {
        val dir = Files.createTempDirectory("keel-print-loop")
        try {
            val device = newDevice()
            val config = RegisterConfig(
                path = dir.resolve("store.db").toString(),
                storeKey = device.storeKey,
                device = device.device,
                signingKey = device.signingKey,
                profile = demoProfile(),
                locale = "en-US",
            )
            Register.open(config).use { register ->
                register.signIn(id(0x501))
                val order = register.startOrder(Mode.TAKEOUT).order
                register.addItem(order, id(0x409), emptyList(), 1u)
                register.payCash(order, 1_000)
                runBlocking { body(register) }
            }
        } finally {
            dir.toFile().deleteRecursively()
        }
    }

    /**
     * A printer that takes [jobs] connections, one after the other, each answered as a ready
     * Epson that printed what it was sent, and gives back what each connection brought.
     */
    private fun printer(jobs: Int): Pair<Int, () -> List<ByteArray>> {
        val server = ServerSocket(0)
        val received = mutableListOf<ByteArray>()
        val worker = thread {
            repeat(jobs) {
                server.accept().use { socket ->
                    val bytes = ByteArrayOutputStream()
                    val input = socket.getInputStream()
                    val output = socket.getOutputStream()
                    var answered = 0
                    val buf = ByteArray(4096)
                    while (true) {
                        val n = input.read(buf)
                        if (n < 0) break
                        bytes.write(buf, 0, n)
                        val all = bytes.toByteArray()
                        // DLE EOT: the printer's status, ready.
                        val questions = (0 until all.size - 1).count { all[it] == 0x10.toByte() && all[it + 1] == 0x04.toByte() }
                        while (answered < questions) {
                            output.write(0x12)
                            answered++
                        }
                        // GS ( H: the job's process ID, answered once it printed.
                        val ask = byteArrayOf(0x1D, 0x28, 0x48, 6, 0, 48, 48)
                        val at = (0..all.size - ask.size).firstOrNull { i -> ask.indices.all { all[i + it] == ask[it] } }
                        if (at != null && all.size >= at + 11) {
                            output.write(byteArrayOf(0x37, 0x22) + all.copyOfRange(at + 7, at + 11) + byteArrayOf(0))
                            output.flush()
                            break
                        }
                    }
                    received += bytes.toByteArray()
                }
            }
            server.close()
        }
        return server.localPort to {
            worker.join()
            received
        }
    }

    @Test
    fun a_sales_drawer_and_receipt_reach_the_printer_each_once() = withSale { register ->
        val (port, received) = printer(jobs = 2)
        val kernel = Executors.newSingleThreadExecutor().asCoroutineDispatcher()
        kernel.use {
            val loop = PrintLoop(register, kernel, { PrinterAddress("127.0.0.1", port) }, idle = 50.milliseconds)
            val running = launch(Dispatchers.IO) { loop.run() }
            withTimeout(30_000) {
                while (withContext(kernel) { register.printJobs() }.isNotEmpty()) delay(50)
            }
            running.cancelAndJoin()
        }
        val (drawer, receipt) = received()
        // The drawer's pulse, ESC p, then the receipt, which names the café.
        assertTrue(drawer.toList().windowed(2).any { it == listOf<Byte>(0x1B, 0x70) }, "the drawer kick first")
        assertTrue(String(receipt, Charsets.ISO_8859_1).contains("Keel Caf"), "then the receipt")
    }

    @Test
    fun with_no_printer_set_jobs_wait() = withSale { register ->
        val kernel = Executors.newSingleThreadExecutor().asCoroutineDispatcher()
        kernel.use {
            val loop = PrintLoop(register, kernel, { null }, idle = 20.milliseconds)
            val running = launch(Dispatchers.IO) { loop.run() }
            delay(300)
            running.cancelAndJoin()
            val jobs = withContext(kernel) { register.printJobs() }
            assertEquals(listOf(JobKind.DRAWER, JobKind.RECEIPT), jobs.map { it.kind })
            assertEquals(listOf(JobState.QUEUED, JobState.QUEUED), jobs.map { it.state })
        }
    }
}
