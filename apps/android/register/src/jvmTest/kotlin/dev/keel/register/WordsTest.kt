package dev.keel.register

import java.io.File
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertTrue

/**
 * The register's words in Spanish (ADR-0038): every word the screens ask for has its Spanish,
 * and a word not in the table reads as itself.
 */
class WordsTest {
    @Test
    fun everyWordTheScreensAskForHasItsSpanish() {
        val screens = File("../app/src/main/kotlin/dev/keel/android")
        assertTrue(screens.isDirectory, "the app's sources, from ${File(".").absolutePath}")
        val asked = screens.listFiles()!!.filter { it.extension == "kt" }
            .flatMap { file -> Regex("""\bt\("([^"$]*)"\)""").findAll(file.readText()).map { it.groupValues[1] }.toList() }
            .toSet()
        assertTrue(asked.size > 80, "the screens ask for their words: ${asked.size}")
        val missing = asked.filter { it !in SPANISH }
        assertEquals(emptyList(), missing, "words with no Spanish")
        // And the reasons the register offers.
        val reasons = (CASH_REASONS + DISCOUNT_REASONS + PRICE_REASONS + RETURN_REASONS + STOCK_REASONS + VOID_REASONS).map { it.words }
        assertEquals(emptyList(), reasons.filter { it !in SPANISH }, "reasons with no Spanish")
    }

    /** No word is left out of [SPANISH] by being shown without asking for it: found in review. */
    @Test
    fun theScreensShowNoWordWithoutAskingForIt() {
        val screens = File("../app/src/main/kotlin/dev/keel/android")
        val shown = listOf("RegisterScreen.kt", "StockScreen.kt", "KitchenScreen.kt").flatMap { name ->
            File(screens, name).readLines().filterNot { it.trim().startsWith("//") || it.trim().startsWith("*") || it.trim().startsWith("import") }
                .flatMap { line -> Regex("""(?<!t\()(?<!testTag\()"([A-Z][^"$]{1,})"""").findAll(line).map { it.groupValues[1] }.toList() }
        }
        assertEquals(emptyList(), shown, "words shown without asking for them in the member's language")
    }

    @Test
    fun aWordReadsInTheMembersLanguage() {
        assertEquals("¿Quién trabaja?", say("Who's working?", "es-US"))
        assertEquals("Who's working?", say("Who's working?", "en-US"))
        assertEquals("Who's working?", say("Who's working?", null))
        assertEquals("the kernel's words", say("the kernel's words", "es-US"))
    }
}
