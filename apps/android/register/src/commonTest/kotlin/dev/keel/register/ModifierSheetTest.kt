package dev.keel.register

import kotlin.test.Test
import kotlin.test.assertEquals

class ModifierSheetTest {
    private val till = FakeTill()
    private val latte = till.latte
    private val breakfast = till.breakfast

    /** The place in [item]'s groups of the group [id], the first if it is offered twice. */
    private fun Item.at(id: String) = groups.indexOfFirst { it.group == id }

    private fun ModifierSheet.tap(group: String, modifier: String) = tap(item.at(group), modifier)

    private fun ModifierSheet.shownIds() = shown.map { it.group.group }

    @Test
    fun the_required_groups_come_first_and_a_group_under_a_modifier_shows_once_it_is_chosen() {
        val required = latte.copy(groups = latte.groups.reversed().filter { it.under == null } + latte.groups.last())
        assertEquals(listOf("milk", "syrup", "shots"), ModifierSheet(required).shownIds())
        val sheet = ModifierSheet(latte)
        assertEquals(listOf("milk", "shots", "syrup"), sheet.shownIds())
        assertEquals(listOf("milk", "shots", "syrup", "strength"), sheet.tap("syrup", "vanilla").shownIds())
    }

    @Test
    fun a_required_group_under_an_optional_one_stays_with_it() {
        // Butter is required, but only once toast is chosen; it moves with toast, not before it.
        val sheet = ModifierSheet(breakfast).tap("toast", "rye")
        assertEquals(listOf("side", "toast", "butter", "extra"), sheet.shownIds())
        assertEquals(false, sheet.complete, "a side and a butter must be chosen")
        val chosen = sheet.tap("side", "fries").tap("butter", "salted")
        assertEquals(true, chosen.complete)
        assertEquals(listOf("side", "sauce", "toast", "butter", "extra"), chosen.shownIds())
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
        val shots = latte.at("shots")
        repeat(3) { sheet = sheet.tap(shots, "shot") }
        assertEquals(3, sheet.count(shots, "shot"))
        sheet = sheet.tap(shots, "shot")
        assertEquals(0, sheet.count(shots, "shot"), "a full group's next tap clears it")
    }

    @Test
    fun a_group_without_repeats_takes_no_more_than_its_most() {
        val syrup = latte.at("syrup")
        val both = ModifierSheet(latte).tap(syrup, "vanilla").tap(syrup, "caramel")
        assertEquals(2, both.chosenIn(syrup))
        val again = both.tap(syrup, "caramel")
        assertEquals(1, again.chosenIn(syrup), "tapping a chosen one clears it")
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
        assertEquals(0, cleared.count(latte.at("strength"), "double"))
    }

    /** Found in review: a modifier chosen in one place was chosen in every place its group is. */
    @Test
    fun a_group_offered_in_two_places_is_chosen_in_one() {
        val underFries = 1
        val underHash = 2
        val fries = ModifierSheet(breakfast).tap("side", "fries").tap(underFries, "ketchup")
        assertEquals(1, fries.count(underFries, "ketchup"))
        assertEquals(0, fries.count(underHash, "ketchup"), "the hash brown's sauce is its own")
        assertEquals(listOf(Chosen("fries", 1), Chosen("ketchup", 1, parent = 0)), fries.choices())
        // The side changed: the fries go, and with them their sauce; the hash brown's is offered.
        val hash = fries.tap("side", "hash")
        assertEquals(listOf("side", "sauce", "toast", "extra"), hash.shownIds())
        assertEquals(underHash, hash.shown[1].index)
        assertEquals(0, hash.count(underFries, "ketchup"), "cleared with the fries")
        assertEquals(
            listOf(Chosen("hash", 1), Chosen("mayo", 1, parent = 0)),
            hash.tap(underHash, "mayo").choices(),
        )
    }

    /** One modifier offered in two groups: what is under each is under it alone. */
    @Test
    fun a_modifier_in_two_groups_offers_what_is_under_it_in_each_place_alone() {
        val extra = breakfast.at("extra")
        val extraSauce = breakfast.groups.lastIndex
        val sheet = ModifierSheet(breakfast).tap("side", "hash").tap(extra, "fries")
        // The extra fries' sauce shows, the side's fries' doesn't: the side is the hash brown.
        assertEquals(listOf(0, 2, 3, extra, extraSauce), sheet.shown.map { it.index })
        val chosen = sheet.tap(extraSauce, "mayo")
        assertEquals(
            listOf(Chosen("hash", 1), Chosen("fries", 1), Chosen("mayo", 1, parent = 1)),
            chosen.choices(),
        )
    }
}
