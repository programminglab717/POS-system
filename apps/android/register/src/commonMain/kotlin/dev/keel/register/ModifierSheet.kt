package dev.keel.register

/**
 * Where a modifier is chosen: the group's place in [Item.groups], which lists each group the item
 * offers followed by the groups under its modifiers, and the modifier. A group offered in two
 * places is two groups there, so a modifier is chosen in one place without the other. Found in
 * review: choices were kept by the modifier alone, and a sauce chosen under fries was chosen
 * under the hash brown too, and charged twice.
 */
data class Slot(val group: Int, val modifier: String)

/** A group the sheet shows, and its place in [Item.groups]. */
data class ShownGroup(val index: Int, val group: Group)

/**
 * The modifier sheet for [item]: how many of each modifier the cashier has chosen, where, and how
 * many of the item to ring. It guides the cashier; the kernel's catalog still checks what is rung.
 */
data class ModifierSheet(
    val item: Item,
    private val counts: Map<Slot, Int> = emptyMap(),
    val quantity: Int = 1,
) {
    /**
     * The slot of the modifier each group of [Item.groups] is offered under, if any: the group
     * above it one level up, whose subtree it is in, and its [Group.under].
     */
    private val parents: List<Slot?> by lazy {
        item.groups.mapIndexed { index, group ->
            val under = group.under ?: return@mapIndexed null
            val parent = (index - 1 downTo 0).firstOrNull { item.groups[it].depth == group.depth - 1 }
            parent?.let { Slot(it, under) }
        }
    }

    /**
     * The groups shown now: the item's own, and those under the modifiers chosen, each followed by
     * the groups under its own modifiers; the item's required groups, and what is under them,
     * first.
     */
    val shown: List<ShownGroup> by lazy {
        val visible = item.groups.indices.filter(::visible).map { ShownGroup(it, item.groups[it]) }
        // Blocks: an item's group, and every group shown after it until the item's next one.
        val blocks = mutableListOf<MutableList<ShownGroup>>()
        for (shown in visible) {
            if (shown.group.under == null || blocks.isEmpty()) blocks += mutableListOf(shown) else blocks.last() += shown
        }
        blocks.sortedBy { block -> if (block.first().group.min > 0) 0 else 1 }.flatten()
    }

    /** Whether every group shown has as many choices as it must and no more than it may. */
    val complete: Boolean get() = shown.all { satisfied(it.index) }

    /** How many of [modifier] are chosen in group [group]. */
    fun count(group: Int, modifier: String): Int = counts[Slot(group, modifier)] ?: 0

    /** How many applications of group [group]'s modifiers are chosen. */
    fun chosenIn(group: Int): Int = item.groups[group].modifiers.sumOf { count(group, it.modifier) }

    /** Whether group [group] has as many choices as it must, and no more than it may. */
    fun satisfied(group: Int): Boolean = item.groups[group].let { chosenIn(group) in it.min..it.max }

    /**
     * The sheet after the cashier taps [modifier] in group [group]: in a group of one, it is chosen
     * alone, or cleared if it was; in a group whose modifiers repeat, one more, until the group is
     * full, and then none; otherwise chosen if the group has room, or cleared if it was.
     */
    fun tap(group: Int, modifier: String): ModifierSheet {
        val offered = item.groups[group]
        val slot = Slot(group, modifier)
        val current = counts[slot] ?: 0
        val room = chosenIn(group) < offered.max
        // A modifier sold out isn't chosen; one chosen before it sold out can be cleared.
        if (current == 0 && offered.modifiers.any { it.modifier == modifier && it.soldOut }) return this
        return when {
            offered.max == 1 && current > 0 -> without(slot)
            offered.max == 1 -> offered.modifiers.fold(this) { sheet, other -> sheet.without(Slot(group, other.modifier)) }.with(slot, 1)
            offered.repeat && room -> with(slot, current + 1)
            current > 0 -> without(slot)
            room -> with(slot, 1)
            else -> this
        }
    }

    /**
     * The sheet for [item] read again, as what is sold out changed (ADR-0033): what was chosen
     * stays, but for a modifier sold out now, and what is under it. An item whose groups changed
     * starts again.
     */
    fun refreshed(item: Item): ModifierSheet {
        fun shape(of: Item) = of.groups.map { group -> Triple(group.group, group.under, group.modifiers.map { it.modifier }) }
        if (shape(item) != shape(this.item)) return ModifierSheet(item, quantity = quantity)
        var sheet = copy(item = item)
        for (slot in counts.keys) {
            if (item.groups[slot.group].modifiers.any { it.modifier == slot.modifier && it.soldOut }) sheet = sheet.without(slot)
        }
        return sheet
    }

    /** The sheet with [quantity] of the item, at least 1. */
    fun withQuantity(quantity: Int): ModifierSheet = copy(quantity = quantity.coerceAtLeast(1))

    /** The choices, flattened as the kernel takes them: each after the choice it is under. */
    fun choices(): List<Chosen> {
        val chosen = mutableListOf<Chosen>()
        val index = mutableMapOf<Slot, Int>()
        for ((group, offered) in shown) {
            for (modifier in offered.modifiers) {
                val slot = Slot(group, modifier.modifier)
                val n = counts[slot] ?: 0
                if (n == 0) continue
                chosen += Chosen(modifier.modifier, n, parents[group]?.let(index::get))
                index[slot] = chosen.size - 1
            }
        }
        return chosen
    }

    /** Whether group [group] is shown: the item's own, or under a modifier chosen in a group shown. */
    private fun visible(group: Int): Boolean {
        val parent = parents[group] ?: return item.groups[group].under == null
        return (counts[parent] ?: 0) > 0 && visible(parent.group)
    }

    private fun with(slot: Slot, n: Int) = copy(counts = counts + (slot to n))

    /** The sheet without [slot]'s choice, nor anything chosen under it. */
    private fun without(slot: Slot): ModifierSheet {
        var sheet = copy(counts = counts - slot)
        for (group in item.groups.indices.filter { parents[it] == slot }) {
            for (under in item.groups[group].modifiers) sheet = sheet.without(Slot(group, under.modifier))
        }
        return sheet
    }
}
