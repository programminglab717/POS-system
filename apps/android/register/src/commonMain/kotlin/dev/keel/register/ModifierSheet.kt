package dev.keel.register

/**
 * The modifier sheet for [item]: how many of each modifier the cashier has chosen, and how many of
 * the item to ring. It guides the cashier; the kernel's catalog still checks what is rung.
 *
 * A modifier's identifier is taken as unique within its item, as the catalog's are.
 */
data class ModifierSheet(
    val item: Item,
    private val counts: Map<String, Int> = emptyMap(),
    val quantity: Int = 1,
) {
    /**
     * The groups shown now: the item's own, and those under the modifiers chosen, each followed by
     * the groups under its own modifiers; the item's required groups, and what is under them,
     * first.
     */
    val shown: List<Group> by lazy {
        val visible = item.groups.filter { group -> group.under == null || count(group.under) > 0 }
        // Blocks: an item's group, and every group shown after it until the item's next one.
        val blocks = mutableListOf<MutableList<Group>>()
        for (group in visible) {
            if (group.under == null || blocks.isEmpty()) blocks += mutableListOf(group) else blocks.last() += group
        }
        blocks.sortedBy { block -> if (block.first().min > 0) 0 else 1 }.flatten()
    }

    /** Whether every group shown has as many choices as it must and no more than it may. */
    val complete: Boolean get() = shown.all(::satisfied)

    /** How many of [modifier] are chosen. */
    fun count(modifier: String): Int = counts[modifier] ?: 0

    /** How many applications of [group]'s modifiers are chosen. */
    fun chosenIn(group: Group): Int = group.modifiers.sumOf { count(it.modifier) }

    /** Whether [group] has as many choices as it must, and no more than it may. */
    fun satisfied(group: Group): Boolean = chosenIn(group) in group.min..group.max

    /**
     * The sheet after the cashier taps [modifier] in [group]: in a group of one, it is chosen
     * alone, or cleared if it was; in a group whose modifiers repeat, one more, until the group is
     * full, and then none; otherwise chosen if the group has room, or cleared if it was.
     */
    fun tap(group: Group, modifier: Modifier): ModifierSheet {
        val id = modifier.modifier
        val current = count(id)
        val room = chosenIn(group) < group.max
        return when {
            group.max == 1 && current > 0 -> without(id)
            group.max == 1 -> group.modifiers.fold(this) { sheet, other -> sheet.without(other.modifier) }.with(id, 1)
            group.repeat && room -> with(id, current + 1)
            current > 0 -> without(id)
            room -> with(id, 1)
            else -> this
        }
    }

    /** The sheet with [quantity] of the item, at least 1. */
    fun withQuantity(quantity: Int): ModifierSheet = copy(quantity = quantity.coerceAtLeast(1))

    /** The choices, flattened as the kernel takes them: each after the choice it is under. */
    fun choices(): List<Chosen> {
        val chosen = mutableListOf<Chosen>()
        val index = mutableMapOf<String, Int>()
        for (group in shown) {
            for (modifier in group.modifiers) {
                val n = count(modifier.modifier)
                if (n == 0) continue
                chosen += Chosen(modifier.modifier, n, group.under?.let(index::get))
                index[modifier.modifier] = chosen.size - 1
            }
        }
        return chosen
    }

    private fun with(modifier: String, n: Int) = copy(counts = counts + (modifier to n))

    /** The sheet without [modifier], nor anything chosen under it. */
    private fun without(modifier: String): ModifierSheet {
        var sheet = copy(counts = counts - modifier)
        for (group in item.groups.filter { it.under == modifier }) {
            for (under in group.modifiers) sheet = sheet.without(under.modifier)
        }
        return sheet
    }
}
