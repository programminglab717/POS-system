// The Kotlin bindings against the kernel built for this host (ADR-0025, decision 5): a register
// on the demo café's profile rings an order with modifiers, refuses what it should as
// exceptions, takes cash, and sends the drawer kick and the receipt to a fake printer.

import dev.keel.kernel.Choice
import dev.keel.kernel.Confirm
import dev.keel.kernel.JobKind
import dev.keel.kernel.JobState
import dev.keel.kernel.KeelException
import dev.keel.kernel.Mode
import dev.keel.kernel.Outcome
import dev.keel.kernel.Prefix
import dev.keel.kernel.Printer
import dev.keel.kernel.Register
import dev.keel.kernel.RegisterConfig
import dev.keel.kernel.TicketState
import dev.keel.kernel.Waits
import dev.keel.kernel.demoProfile
import dev.keel.kernel.printTcp
import java.io.ByteArrayOutputStream
import java.net.ServerSocket
import java.nio.file.Files
import kotlin.concurrent.thread
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFailsWith
import kotlin.test.assertNull
import kotlin.test.assertTrue

private fun id(n: Long) = "0192f0c1-0000-7000-8000-%012x".format(n)

/** A register on the demo profile, in a directory of its own, closed and removed after [body]. */
private fun withRegister(body: (Register) -> Unit) {
    val dir = Files.createTempDirectory("keel-kotlin")
    try {
        val config = RegisterConfig(
            path = dir.resolve("store.db").toString(),
            storeKey = ByteArray(32) { 0x4b },
            device = id(0xd1),
            signingKey = ByteArray(32) { 7 },
            profile = demoProfile(),
            locale = "en-US",
        )
        Register.open(config).use { register ->
            // A store opens once at a time: a second register is refused until this one closes.
            assertFailsWith<KeelException.StoreInUse> { Register.open(config) }
            body(register)
        }
    } finally {
        dir.toFile().deleteRecursively()
    }
}

/** A printer that takes one connection, answers as a ready Epson, and gives back what it got. */
private fun printer(): Pair<Int, () -> ByteArray> {
    val server = ServerSocket(0)
    val received = ByteArrayOutputStream()
    val worker = thread {
        server.accept().use { socket ->
            val input = socket.getInputStream()
            val output = socket.getOutputStream()
            var answered = 0
            val buf = ByteArray(4096)
            while (true) {
                val n = input.read(buf)
                if (n < 0) break
                received.write(buf, 0, n)
                val all = received.toByteArray()
                val questions = (0 until all.size - 1).count { all[it] == 0x10.toByte() && all[it + 1] == 0x04.toByte() }
                while (answered < questions) {
                    output.write(0x12)
                    answered++
                }
                val ask = byteArrayOf(0x1D, 0x28, 0x48, 6, 0, 48, 48)
                val at = (0..all.size - ask.size).firstOrNull { i -> ask.indices.all { all[i + it] == ask[it] } }
                if (at != null && all.size >= at + 11) {
                    output.write(byteArrayOf(0x37, 0x22) + all.copyOfRange(at + 7, at + 11) + byteArrayOf(0))
                    output.flush()
                    break
                }
            }
        }
        server.close()
    }
    return server.localPort to {
        worker.join()
        received.toByteArray()
    }
}

class RegisterTest {
    @Test
    fun aSaleRingsPaysAndPrints() = withRegister { register ->
        assertFailsWith<KeelException.NotSignedIn> { register.startOrder(Mode.TAKEOUT) }
        register.signIn(id(0x501))
        val menu = register.menu()
        assertEquals("Coffee", menu.pages[0].name)
        assertEquals("$5.25", menu.pages[0].buttons[2].price.text)

        val order = register.startOrder(Mode.TAKEOUT).order
        // The catalog's refusals reach Kotlin as exceptions: a sandwich needs its egg.
        assertFailsWith<KeelException.Catalog> { register.addItem(order, id(0x408), emptyList(), 1u) }
        // A large latte with oat milk and two extra shots; a sandwich, fried, with a hash brown
        // and ketchup under it.
        val latte = listOf(Choice(id(0x202), 1u, null), Choice(id(0x205), 2u, null))
        register.addItem(order, id(0x403), latte, 1u)
        val sandwich = listOf(Choice(id(0x20e), 1u, null), Choice(id(0x210), 1u, null), Choice(id(0x211), 1u, 1u))
        val ticket = register.addItem(order, id(0x408), sandwich, 1u)
        assertEquals(listOf("Latte (Large)", "Breakfast sandwich"), ticket.lines.map { it.name })
        assertEquals(listOf(Prefix.PLAIN, Prefix.EXTRA), ticket.lines[0].modifiers.map { it.prefix })
        assertEquals(listOf(1, 1, 2), ticket.lines[1].modifiers.map { it.depth.toInt() })

        val short = assertFailsWith<KeelException.TenderShort> { register.payCash(order, 100) }
        assertEquals(ticket.total.minor, short.due.minor)
        assertEquals(ticket.total.text, short.due.text)
        assertFailsWith<KeelException.NegativeTender> { register.payCash(order, -1) }
        val paid = register.payCash(order, 5_000)
        assertEquals(TicketState.CLOSED, paid.ticket.state)
        assertEquals(5_000 - ticket.total.minor, paid.change.minor)

        assertEquals(listOf(JobKind.DRAWER, JobKind.RECEIPT), register.printJobs().map { it.kind })
        register.setPrinter(Printer(48u, true, Confirm.PRINTED))
        val waits = Waits(2_000u, 5_000u)
        val printed = mutableListOf<JobKind>()
        while (true) {
            val job = register.nextPrintJob() ?: break
            val (port, received) = printer()
            val outcome = printTcp("127.0.0.1", port.toUShort(), job, waits)
            assertEquals(Outcome.Printed(false), outcome)
            assertTrue(received().toList().windowed(job.bytes.size).any { it == job.bytes.toList() })
            register.printOutcome(job.job, outcome)
            printed += job.kind
        }
        assertEquals(listOf(JobKind.DRAWER, JobKind.RECEIPT), printed)
        assertTrue(register.printJobs().isEmpty())

        // A reprint, with no printer listening: not sent, and queued again.
        register.reprint(order)
        val job = register.nextPrintJob()!!
        val closed = ServerSocket(0).use { it.localPort }
        val outcome = printTcp("127.0.0.1", closed.toUShort(), job, waits)
        assertTrue(outcome is Outcome.NotSent)
        register.printOutcome(job.job, outcome)
        assertEquals(JobState.QUEUED, register.printJobs().single().state)
        assertNull(register.nextPrintJob())
        assertFailsWith<KeelException.NotUnconfirmed> { register.resolve(job.job, dev.keel.kernel.Resolution.PRINTED) }
    }

    @Test
    fun badArgumentsAreRefused() = withRegister { register ->
        register.signIn(id(0x501))
        assertFailsWith<KeelException.InvalidArgument> { register.ticket("not an identifier") }
        assertFailsWith<KeelException.UnknownOrder> { register.ticket(id(0x999)) }
        val order = register.startOrder(Mode.DINE_IN).order
        assertFailsWith<KeelException.InvalidArgument> { register.addItem(order, id(0x401), emptyList(), 0u) }
        assertFailsWith<KeelException.InvalidArgument> {
            register.addItem(order, id(0x401), listOf(Choice(id(0x205), 1u, 0u)), 1u)
        }
        assertFailsWith<KeelException.InvalidArgument> {
            Register.open(RegisterConfig("x", ByteArray(31), id(1), ByteArray(32), demoProfile(), "en-US"))
        }
    }
}
