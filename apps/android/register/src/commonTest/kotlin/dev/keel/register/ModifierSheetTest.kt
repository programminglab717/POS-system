package dev.keel.register

import kotlin.test.Test
import kotlin.test.assertEquals

class ModifierSheetTest {
    private val latte = FakeTill().latte
    private fun group(id: String) = latte.groups.single { it.group == id }
    private fun ModifierSheet.tap(group: String, modifier: String) =
        tap(group(group), group(group).modifiers.single { it.modifier == modifier })

    @Test
    fun the_required_groups_come_first_and_a_group_under_a_modifier_shows_once_it_is_chosen() {
        val required = latte.copy(groups = latte.groups.reversed().filter { it.under == null } + latte.groups.last())
        assertEquals(listOf("milk", "syrup", "shots"), ModifierSheet(required).shown.map { it.group })
        val sheet = ModifierSheet(latte)
        assertEquals(listOf("milk", "shots", "syrup"), sheet.shown.map { it.group })
        assertEquals(listOf("milk", "shots", "syrup", "strength"), sheet.tap("syrup", "vanilla").shown.map { it.group })
    }

    @Test
    fun a_group_of_one_chooses_alone_and_a_second_tap_clears() {
        val oat = ModifierSheet(latte).tap("milk", "whole").tap("milk", "oat")
        assertEquals(listOf(Chosen("oat", 1)), oat.choices())
        assertEquals(true, oat.complete)
        val none = oat.tap("milk", "oat")
        assertEquals(emptyList(), none.choices())
        assertEquals(false, none.complete)
    }

    @Test
    fun repeated_modifiers_count_up_to_the_groups_most_then_clear() {
        var sheet = ModifierSheet(latte)
        repeat(3) { sheet = sheet.tap("shots", "shot") }
        assertEquals(3, sheet.count("shot"))
        sheet = sheet.tap("shots", "shot")
        assertEquals(0, sheet.count("shot"), "a full group's next tap clears it")
    }

    @Test
    fun a_group_without_repeats_takes_no_more_than_its_most() {
        val both = ModifierSheet(latte).tap("syrup", "vanilla").tap("syrup", "caramel")
        assertEquals(2, both.chosenIn(group("syrup")))
        val again = both.tap("syrup", "caramel")
        assertEquals(1, again.chosenIn(group("syrup")), "tapping a chosen one clears it")
    }

    @Test
    fun choices_under_a_modifier_name_its_choice_and_go_with_it() {
        val sheet = ModifierSheet(latte).tap("milk", "oat").tap("syrup", "vanilla").tap("strength", "double").withQuantity(0)
        assertEquals(
            listOf(Chosen("oat", 1), Chosen("vanilla", 1), Chosen("double", 1, parent = 1)),
            sheet.choices(),
        )
        assertEquals(1, sheet.quantity, "at least one")
        val cleared = sheet.tap("syrup", "vanilla")
        assertEquals(listOf(Chosen("oat", 1)), cleared.choices(), "clearing a modifier clears what was chosen under it")
        assertEquals(0, cleared.count("double"))
    }
}
