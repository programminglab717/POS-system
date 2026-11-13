package dev.keel.register

/**
 * A till over a café of three items: a latte, whose milk must be chosen and whose shots and
 * syrups are optional, a syrup offering a flavor shot under it; tea; and scones. Prices in cents;
 * no tax; cash owed is what was rung.
 */
class FakeTill : Till {
    var signedIn: String? = null
    val rung = mutableListOf<Pair<String, List<Chosen>>>()
    private val prices = mapOf("latte" to 450L, "tea" to 300L, "scone" to 275L)
    private val orders = mutableMapOf<String, MutableList<Line>>()
    private val states = mutableMapOf<String, OrderState>()
    private var lines = 0
    /** The print jobs, as a printer that never answers leaves them. */
    val jobs = mutableListOf<PrintJob>()

    /** Each order's checks: its own, first, and any opened since; and what each was paid. */
    private val checks = mutableMapOf<String, MutableList<String>>()
    private val checkPaid = mutableMapOf<String, Long>()
    private val closedChecks = mutableSetOf<String>()

    private class Line(val id: String, val variant: String, var count: Int) {
        /** The checks it is split evenly among; its order's own check when empty. */
        var on: List<String> = emptyList()
        /** The price staff set, in cents, and why, if they did. */
        var price: Pair<Long, String>? = null
        var notes: String? = null
    }

    /** Whether pricing by hand needs a manager, as for a cashier. */
    var priceNeedsApproval = false

    /** A discount, on a line or on the whole order. */
    private class Off(val id: String, val line: String?, val asked: DiscountAsked, val reason: String)

    private val discounts = mutableMapOf<String, MutableList<Off>>()
    private var discountsMinted = 0

    /** The most a cashier takes off without a manager, in basis points; none means no limit. */
    var discountLimit: Int? = null

    /** The discounts asked for, as the till got them. */
    val asked = mutableListOf<Pair<String?, DiscountAsked>>()

    val latte = Item(
        "latte", "Latte", "$4.50",
        listOf(
            Group("milk", null, "Milk", 1, 1, 0, false, listOf(Modifier("whole", "Whole", "$0.00"), Modifier("oat", "Oat", "$0.75"))),
            Group("shots", null, "Extra shots", 0, 3, 0, true, listOf(Modifier("shot", "Shot", "$0.95"))),
            Group("syrup", null, "Syrup", 0, 2, 0, false, listOf(Modifier("vanilla", "Vanilla", "$0.60"), Modifier("caramel", "Caramel", "$0.60"))),
            Group("strength", "vanilla", "Vanilla strength", 0, 1, 0, false, listOf(Modifier("double", "Double", "$0.30"))),
        ),
    )

    /**
     * A breakfast whose side must be chosen, fries or a hash brown, each offering the same sauce
     * group under it; toast, optional, whose butter must be chosen once toast is; and fries
     * again, as an extra, with a sauce of their own: one modifier in two groups.
     */
    val breakfast = Item(
        "breakfast", "Breakfast", "$6.95",
        listOf(
            Group("side", null, "Side", 1, 1, 0, false, listOf(Modifier("fries", "Fries", "$0.00"), Modifier("hash", "Hash brown", "$0.00"))),
            Group("sauce", "fries", "Sauce", 0, 1, 0, false, listOf(Modifier("ketchup", "Ketchup", "$0.00"), Modifier("mayo", "Mayo", "$0.25")), depth = 2),
            Group("sauce", "hash", "Sauce", 0, 1, 0, false, listOf(Modifier("ketchup", "Ketchup", "$0.00"), Modifier("mayo", "Mayo", "$0.25")), depth = 2),
            Group("toast", null, "Toast", 0, 1, 0, false, listOf(Modifier("rye", "Rye", "$0.50"))),
            Group("butter", "rye", "Butter", 1, 1, 0, false, listOf(Modifier("salted", "Salted", "$0.00"), Modifier("none", "No butter", "$0.00")), depth = 2),
            // Fries again, as an extra, with a sauce of their own.
            Group("extra", null, "Extra", 0, 1, 0, false, listOf(Modifier("fries", "Fries", "$2.50"))),
            Group("sauce", "fries", "Sauce", 0, 1, 0, false, listOf(Modifier("ketchup", "Ketchup", "$0.00"), Modifier("mayo", "Mayo", "$0.25")), depth = 2),
        ),
    )

    override fun team() = listOf(Member("alex", "Alex"), Member("sam", "Sam"))

    /** Each member's PIN, for a till whose team has them; none for the plain team. */
    var pins: Map<String, String> = emptyMap()

    /** Whether dropping an order with items on it needs a manager, as for a cashier. */
    var voidNeedsApproval = false

    /** The approver waiting, once one has approved. */
    var approvedBy: String? = null

    override fun signIn(member: String, pin: String) {
        if (team().none { it.id == member }) throw TillException("no such member")
        if (pin != pins[member].orEmpty()) throw TillException("That PIN isn't right.")
        signedIn = member
    }

    override fun approvers() =
        if (voidNeedsApproval || discountLimit != null || priceNeedsApproval || paidOutLimit != null ||
            noSaleNeedsApproval || varianceLimit != null || refundLimit != null || markNeedsApproval
        ) {
            listOf("alex")
        } else {
            emptyList()
        }

    override fun approve(approver: String, pin: String) {
        if (pin != pins[approver].orEmpty()) throw TillException("That PIN isn't right.")
        approvedBy = approver
    }

    override fun signOut() {
        signedIn = null
    }

    /** What is sold out, each with when it is back: null until put back (ADR-0033). */
    val sold = mutableMapOf<String, String?>()

    /** Moves with every change of what is sold out, as the kernel's version does. */
    var availability = 0L

    /** Whether an 86 needs a manager, as for a cashier whose role doesn't permit it. */
    var markNeedsApproval = false

    /** Where an 86 for the day ends, as the kernel says it. */
    var endOfDay = "2026-10-03 04:00"

    private val names = mapOf(
        "latte" to "Latte", "tea" to "Tea", "scone" to "Scone", "whole" to "Whole", "oat" to "Oat",
        "shot" to "Shot", "vanilla" to "Vanilla", "caramel" to "Caramel", "double" to "Double",
    )

    private val modifierIds = setOf("whole", "oat", "shot", "vanilla", "caramel", "double")

    override fun menu() = listOf(
        MenuPage("Drinks", listOf(MenuButton("latte", "Latte", "$4.50", choices = true, required = true), MenuButton("tea", "Tea", "$3.00"))),
        MenuPage("Food", listOf(MenuButton("scone", "Scone", "$2.75"))),
    ).map { page ->
        page.copy(buttons = page.buttons.map { it.copy(soldOut = it.variant in sold, backAt = sold[it.variant]) })
    }

    override fun item(variant: String): Item {
        if (variant != "latte") throw TillException("no choices")
        return latte.copy(
            groups = latte.groups.map { group ->
                group.copy(modifiers = group.modifiers.map { it.copy(soldOut = it.modifier in sold, backAt = sold[it.modifier]) })
            },
        )
    }

    override fun eightySix(item: String, modifier: Boolean, untilPutBack: Boolean) {
        check(signedIn != null) { "no one signed in" }
        val known = if (modifier) item in modifierIds else item in prices
        if (!known) throw TillException("the catalog has no item $item")
        val back = if (untilPutBack) null else endOfDay
        if (item in sold && sold[item] == back) throw TillException("the item is already sold out until then, or already on sale")
        if (markNeedsApproval && approvedBy == null) throw TillException("Marking an item out needs a manager's approval.", approval = "item.availability")
        approvedBy = null
        sold[item] = back
        availability += 1
    }

    override fun putBack(item: String, modifier: Boolean) {
        check(signedIn != null) { "no one signed in" }
        if (item !in sold) throw TillException("the item is already sold out until then, or already on sale")
        if (markNeedsApproval && approvedBy == null) throw TillException("Marking an item out needs a manager's approval.", approval = "item.availability")
        approvedBy = null
        sold -= item
        availability += 1
    }

    override fun soldOut() = sold.keys.sorted().map { SoldOutItem(it, it in modifierIds, names.getValue(it), sold[it]) }

    override fun availabilityVersion(): Long? = availability

    override fun startOrder(): Ticket {
        check(signedIn != null) { "no one signed in" }
        val order = "order${orders.size + 1}"
        orders[order] = mutableListOf()
        states[order] = OrderState.OPEN
        return ticket(order)
    }

    override fun addItem(order: String, variant: String, choices: List<Chosen>, quantity: Int): Ticket {
        prices[variant] ?: throw TillException("not on the menu")
        // The kernel refuses what is sold out, the item or a modifier chosen (ADR-0033).
        (listOf(variant) + choices.map { it.modifier }).firstOrNull { it in sold }?.let {
            throw TillException("${names.getValue(it)} is sold out")
        }
        if (variant == "latte" && choices.none { it.modifier == "whole" || it.modifier == "oat" }) {
            throw TillException("choose a milk")
        }
        rung += variant to choices
        lines += 1
        open(order) += Line("line$lines", variant, quantity)
        return ticket(order)
    }

    override fun changeQuantity(order: String, line: String, quantity: Int): Ticket {
        open(order).single { it.id == line }.count = quantity
        return ticket(order)
    }

    override fun removeLine(order: String, line: String): Ticket {
        open(order).removeAll { it.id == line }
        return ticket(order)
    }

    override fun applyDiscount(order: String, line: String?, discount: DiscountAsked, reason: String): Ticket {
        open(order)
        if (line != null && orders.getValue(order).none { it.id == line }) throw TillException("no such line")
        if (line == null && discount is DiscountAsked.Amount) throw TillException("a whole order takes a percentage")
        val limit = discountLimit
        if (limit != null && discount is DiscountAsked.Percent && discount.basisPoints > limit) {
            if (approvedBy == null) throw TillException("That discount needs a manager's approval.", approval = "discount.apply")
            approvedBy = null
        }
        asked += line to discount
        discountsMinted += 1
        discounts.getOrPut(order) { mutableListOf() } += Off("discount$discountsMinted", line, discount, reason)
        return ticket(order)
    }

    override fun removeDiscount(order: String, discount: String): Ticket {
        open(order)
        if (discounts[order]?.removeAll { it.id == discount } != true) throw TillException("no such discount")
        return ticket(order)
    }

    override fun repriceLine(order: String, line: String, unitPriceMinor: Long, reason: String): Ticket {
        val found = open(order).singleOrNull { it.id == line } ?: throw TillException("no such line")
        if (unitPriceMinor < 0) throw TillException("A price can't be less than nothing.")
        if (unitPriceMinor == unitPrice(found)) throw TillException("That's its price already.")
        if (priceNeedsApproval) {
            if (approvedBy == null) throw TillException("Changing a price needs a manager's approval.", approval = "price.override")
            approvedBy = null
        }
        found.price = unitPriceMinor to reason
        return ticket(order)
    }

    override fun setNotes(order: String, line: String, notes: String?): Ticket {
        val found = open(order).singleOrNull { it.id == line } ?: throw TillException("no such line")
        if (found.notes == notes) throw TillException("Those are its notes already.")
        found.notes = notes
        return ticket(order)
    }

    private fun unitPrice(line: Line) = line.price?.first ?: prices.getValue(line.variant)

    override fun splitEvenly(order: String, ways: Int): Ticket {
        val lines = open(order)
        if (ways < 2 || lines.isEmpty()) throw TillException("Nothing to split that way.")
        val known = checks.getOrPut(order) { mutableListOf(order) }
        while (known.size < ways) known += "$order:check${known.size + 1}"
        for (line in lines) line.on = known.take(ways)
        return ticket(order)
    }

    override fun moveLine(order: String, line: String, to: String?): Ticket {
        val found = open(order).singleOrNull { it.id == line } ?: throw TillException("no such line")
        val known = checks.getOrPut(order) { mutableListOf(order) }
        val target = to ?: "$order:check${known.size + 1}".also { known += it }
        if (target !in known) throw TillException("no such check")
        found.on = listOf(target)
        return ticket(order)
    }

    override fun payCheckCash(order: String, check: String, partMinor: Long?, tenderedMinor: Long): Paid {
        open(order)
        val owed = checkDue(order, check)
        val paying = partMinor ?: owed
        if (paying <= 0 || paying > owed) throw TillException("That's more than the check owes.")
        if (tenderedMinor < paying) throw TillException("That's short: ${money(paying)} is due.")
        checkPaid[check] = (checkPaid[check] ?: 0) + paying
        if (checkDue(order, check) == 0L) closedChecks += check
        val holding = checks.getValue(order).filter { known -> lineParts(order).any { it.second == known } }
        if (holding.all { it in closedChecks }) states[order] = OrderState.CLOSED
        return Paid(money(tenderedMinor - paying), ticket(order))
    }

    /** Each line's part on each check it is on: the line's id, the check, and the part in cents. */
    private fun lineParts(order: String): List<Triple<String, String, Long>> = orders.getValue(order).flatMap { line ->
        val on = line.on.ifEmpty { listOf(order) }
        val whole = unitPrice(line) * line.count
        on.mapIndexed { index, check ->
            Triple(line.id, check, whole / on.size + if (index == 0) whole % on.size else 0)
        }
    }

    private fun checkTotal(order: String, check: String) = lineParts(order).filter { it.second == check }.sumOf { it.third }

    private fun checkDue(order: String, check: String) = checkTotal(order, check) - (checkPaid[check] ?: 0)

    /** This till's device, and each order's owner and the devices that asked for it. */
    var device = "here"
    val owners = mutableMapOf<String, String>()
    val askers = mutableMapOf<String, MutableList<String>>()

    /** Whether the hub grants a recall at once, as one that answers quickly does. */
    var hubAnswers = true

    override fun deviceId() = device

    override fun recall(order: String): Ticket {
        if (owners[order] == device) return ticket(order)
        if (hubAnswers) owners[order] = device else askers.getOrPut(order) { mutableListOf() } += device
        return ticket(order)
    }

    override fun takeOver(order: String): Ticket {
        if (owners[order] == device) throw TillException("This register has it already.")
        if (voidNeedsApproval) {
            if (approvedBy == null) throw TillException("Taking it over needs a manager.", approval = "order.void")
            approvedBy = null
        }
        owners[order] = device
        askers.remove(order)
        return ticket(order)
    }

    /** The drawer: its float and movements in cents, and its close; and what pays out without a manager. */
    private var drawerFloat: Long? = null
    private var drawerIn = 0L
    private var drawerOut = 0L
    private var drawerDropped = 0L
    private var drawerRefunded = 0L
    private var drawerSales = 0L
    private var drawerNoSales = 0L
    var paidOutLimit: Long? = null

    /** The reason the last movement or no-sale was recorded with. */
    var lastCashReason: String? = null

    /** Whether a no-sale needs a manager, as `drawer.no_sale` does for a role without it. */
    var noSaleNeedsApproval = false

    /** The most a close may be over or short without a manager, as `drawer.variance`; null for any. */
    var varianceLimit: Long? = null
    val closedDrawers = mutableListOf<Drawer>()

    private fun drawerView(closedWith: Long? = null): Drawer {
        val float = drawerFloat ?: 0
        val expected = float + drawerSales + drawerIn - drawerOut - drawerDropped - drawerRefunded
        // Blind while open, as the kernel's.
        val shown = closedWith != null
        return Drawer(
            open = !shown, float = money(float), taken = money(drawerSales).takeIf { shown },
            paidIn = money(drawerIn).takeIf { shown }, paidOut = money(drawerOut).takeIf { shown },
            dropped = money(drawerDropped).takeIf { shown }, refunded = money(drawerRefunded).takeIf { shown },
            noSales = drawerNoSales,
            expected = closedWith?.let { money(expected) }, counted = closedWith?.let(::money),
            variance = closedWith?.let { money(it - expected) },
        )
    }

    override fun drawer(): Drawer? = drawerFloat?.let { drawerView() }

    override fun openDrawer(floatMinor: Long): Drawer {
        if (drawerFloat != null) throw TillException("The drawer is open already.")
        drawerFloat = floatMinor
        drawerIn = 0; drawerOut = 0; drawerDropped = 0; drawerSales = 0; drawerNoSales = 0; drawerRefunded = 0
        return drawerView()
    }

    override fun moveCash(movement: CashMovement, amountMinor: Long, reason: String): Drawer {
        if (drawerFloat == null) throw TillException("No drawer is open.")
        // As the kernel's drawer: moving nothing, or less, is refused.
        if (amountMinor <= 0) throw TillException("invalid event: an amount moved must be more than zero")
        lastCashReason = reason
        when (movement) {
            CashMovement.PAID_IN -> drawerIn += amountMinor
            CashMovement.DROP -> drawerDropped += amountMinor
            CashMovement.PAID_OUT -> {
                val limit = paidOutLimit
                if (limit != null && amountMinor > limit) {
                    if (approvedBy == null) throw TillException("Paying out needs a manager's approval.", approval = "drawer.paid_out")
                    approvedBy = null
                }
                drawerOut += amountMinor
            }
        }
        return drawerView()
    }

    override fun noSale(reason: String): Drawer {
        if (drawerFloat == null) throw TillException("No drawer is open.")
        if (noSaleNeedsApproval) {
            if (approvedBy == null) throw TillException("A no-sale needs a manager's approval.", approval = "drawer.no_sale")
            approvedBy = null
        }
        drawerNoSales += 1
        lastCashReason = reason
        return drawerView()
    }

    override fun closeDrawer(countedMinor: Long): Drawer {
        if (drawerFloat == null) throw TillException("No drawer is open.")
        val closed = drawerView(countedMinor)
        val limit = varianceLimit
        val variance = countedMinor - ((drawerFloat ?: 0) + drawerSales + drawerIn - drawerOut - drawerDropped - drawerRefunded)
        if (limit != null && kotlin.math.abs(variance) > limit) {
            if (approvedBy == null) throw TillException("Closing over or short needs a manager's approval.", approval = "drawer.variance")
            approvedBy = null
        }
        closedDrawers += closed
        drawerFloat = null
        return closed
    }

    override fun dayReport(z: Boolean): DayReport {
        // The kernel's words.
        if (z && drawerFloat != null) throw TillException("a drawer is open: close it first")
        val paid = states.filterValues { it == OrderState.CLOSED }.keys
        return DayReport(
            z = z, date = "2026-11-12", orders = paid.size.toLong(), gross = money(0), discounts = money(0),
            comps = money(0), net = money(0), tax = money(0), total = money(0), tenders = emptyList(), voided = 0,
            drawers = closedDrawers.toList() + listOfNotNull(drawer()),
        )
    }

    override fun printReport(z: Boolean) {
        dayReport(z)
        jobs += PrintJob("report:${if (z) "z" else "x"}", JobKind.RECEIPT, JobState.QUEUED, 0)
    }

    override fun abandon(order: String): Ticket {
        if (voidNeedsApproval && open(order).isNotEmpty()) {
            if (approvedBy == null) throw TillException("Dropping it needs a manager.", approval = "order.void")
            approvedBy = null
        }
        open(order).clear()
        states[order] = OrderState.ABANDONED
        return ticket(order)
    }

    override fun cashOffers(order: String): List<Amount> {
        val due = due(order)
        return listOf(amount(due), amount((due / 100 + 1) * 100), amount((due / 500 + 1) * 500))
    }

    override fun amount(minor: Long) = Amount(minor, money(minor))

    override fun payCash(order: String, tenderedMinor: Long): Paid {
        val due = due(order)
        if (tenderedMinor < due) throw TillException("That's short: ${money(due)} is due.")
        if (drawerFloat != null) drawerSales += due
        states[order] = OrderState.CLOSED
        jobs += PrintJob("drawer:$order", JobKind.DRAWER, JobState.QUEUED, 0)
        jobs += PrintJob("receipt:$order", JobKind.RECEIPT, JobState.QUEUED, 0)
        return Paid(money(tenderedMinor - due), ticket(order))
    }

    override fun ticket(order: String): Ticket {
        val due = due(order)
        val (lines, onOrder) = priced(order)
        return Ticket(
            order = order,
            lines = orders.getValue(order).map { line ->
                TicketLine(
                    line.id, line.variant, line.count.toString(), money(unitPrice(line) * line.count), line.count,
                    discounts = lines.getValue(line.id).second,
                    repriced = line.price?.let { (price, reason) ->
                        "${money(price)} (was ${money(prices.getValue(line.variant))}), ${reasonWords(reason).lowercase()}"
                    },
                    notes = line.notes,
                )
            },
            total = money(due),
            cashDueMinor = due,
            cashDue = money(due),
            state = states.getValue(order),
            discounts = onOrder.second,
            checks = checksOf(order),
            owner = owners[order] ?: device,
            asking = askers[order].orEmpty(),
        )
    }

    /** The order's checks, if it is split among more than one that hold something. */
    private fun checksOf(order: String): List<TicketCheck> {
        val parts = lineParts(order)
        val holding = checks[order].orEmpty().filter { known -> parts.any { it.second == known } }
        if (holding.size < 2) return emptyList()
        return holding.mapIndexed { index, check ->
            val lines = parts.filter { it.second == check }.map { (line, _, part) ->
                val found = orders.getValue(order).single { it.id == line }
                TicketLine(line, found.variant, found.count.toString(), money(part), found.count)
            }
            val due = checkDue(order, check)
            TicketCheck(check, index + 1, check !in closedChecks, lines, money(checkTotal(order, check)), money(due), due, money(due))
        }
    }

    /** Each line's net and discounts, and the whole order's net and discounts: each takes from what is left. */
    private fun priced(order: String): Pair<Map<String, Pair<Long, List<TicketDiscount>>>, Pair<Long, List<TicketDiscount>>> {
        val offs = discounts[order].orEmpty()
        fun take(left: Long, off: Off): Long = when (val asked = off.asked) {
            is DiscountAsked.Percent -> (left * asked.basisPoints + 5_000) / 10_000
            is DiscountAsked.Amount -> minOf(asked.minor, left)
        }
        fun words(off: Off) = reasonWords(off.reason) +
            ((off.asked as? DiscountAsked.Percent)?.let { " " + percentWords(it.basisPoints) } ?: "")
        val lines = orders.getValue(order).associate { line ->
            var left = unitPrice(line) * line.count
            val taken = offs.filter { it.line == line.id }.map { off ->
                val amount = take(left, off)
                left -= amount
                TicketDiscount(off.id, words(off), "−" + money(amount))
            }
            line.id to (left to taken)
        }
        var left = lines.values.sumOf { it.first }
        val onOrder = offs.filter { it.line == null }.map { off ->
            val amount = take(left, off)
            left -= amount
            TicketDiscount(off.id, words(off), "−" + money(amount))
        }
        return lines to (left to onOrder)
    }

    override fun ordersToday() = orders.keys.filter { it !in earlier }.map(::ticket)

    /** Orders begun on an earlier day: open ones list only among the open orders. */
    val earlier = mutableSetOf<String>()

    override fun openOrders() = orders.keys.filter { states[it] == OrderState.OPEN }.map(::ticket)

    override fun printJobs() = jobs.toList()

    /** How much of each line was returned (ADR-0032). */
    private val returned = mutableMapOf<String, Long>()
    private val refundedOf = mutableMapOf<String, Long>()

    /** The most a refund may be without a manager, as `refund.issue`; null for any. */
    var refundLimit: Long? = null

    /** The reason the last return was made for. */
    var lastReturnReason: String? = null

    /** The orders a card paid, which aren't refunded in cash, as the kernel's. */
    val cardPaid = mutableSetOf<String>()

    /** The items sold by measure: a line of one is returned whole. */
    var measured = setOf<String>()

    // As the kernel's: a code of six characters, trimmed, whatever its case.
    override fun findOrders(code: String): List<String> {
        val trimmed = code.trim().lowercase()
        if (trimmed.length != RECEIPT_CODE) return emptyList()
        return states.filterValues { it == OrderState.CLOSED }.keys.filter { it.endsWith(trimmed) }.reversed()
    }

    override fun returnable(order: String): Returnable {
        if (states[order] != OrderState.CLOSED) throw TillException("the order isn't closed")
        val lines = orders.getValue(order).map { line ->
            val left = line.count.toLong() - (returned[line.id] ?: 0)
            val count = left.takeIf { line.variant !in measured }
            ReturnableLine(line.id, line.variant, left.toString(), count, left > 0, money(unitPrice(line) * line.count))
        }
        return Returnable(order, lines, money(refundedOf[order] ?: 0), cashOnly = order !in cardPaid)
    }

    override fun returnLines(order: String, lines: List<ReturnPick>, reason: String): Refund {
        // As the kernel's order: only a closed one, each line once, no more than is left.
        if (states[order] != OrderState.CLOSED) throw TillException("the order isn't closed")
        if (order in cardPaid) throw TillException("a card paid the order: it is refunded to the card, which isn't built yet")
        if (lines.isEmpty() || lines.map { it.line }.toSet().size != lines.size) throw TillException("invalid event: lines")
        val each = lines.map { pick ->
            val line = orders.getValue(order).find { it.id == pick.line } ?: throw TillException("no line ${pick.line}")
            val left = line.count.toLong() - (returned[line.id] ?: 0)
            val count = pick.count ?: left
            if (count <= 0 || count > left) throw TillException("more of line ${line.id} than is left to return")
            Triple(line, count, unitPrice(line) * count)
        }
        val total = each.sumOf { it.third }
        val limit = refundLimit
        if (limit != null && total > limit) {
            if (approvedBy == null) throw TillException("A refund needs a manager's approval.", approval = "refund.issue")
            approvedBy = null
        }
        for ((line, count, _) in each) returned[line.id] = (returned[line.id] ?: 0) + count
        refundedOf[order] = (refundedOf[order] ?: 0) + total
        if (drawerFloat != null) drawerRefunded += total
        lastReturnReason = reason
        jobs += PrintJob("drawer:refund:$order", JobKind.DRAWER, JobState.QUEUED, 0)
        jobs += PrintJob("refund:$order", JobKind.RECEIPT, JobState.QUEUED, 0)
        return Refund(money(total), money(total), each.map { (line, count, amount) -> "$count ${line.variant}" to money(amount) })
    }

    override fun resolve(job: String, printed: Boolean) {
        val index = jobs.indexOfFirst { it.job == job && it.state == JobState.UNCONFIRMED }
        if (index < 0) throw TillException("That job isn't waiting for a decision.")
        val unconfirmed = jobs.removeAt(index)
        if (!printed) jobs += unconfirmed.copy(job = "${unconfirmed.job}:again", state = JobState.QUEUED, attempts = 0)
    }

    override fun reprint(order: String) {
        if (states[order] != OrderState.CLOSED) throw TillException("no receipt")
        jobs += PrintJob("reprint:$order", JobKind.RECEIPT, JobState.QUEUED, 0)
    }

    private fun open(order: String): MutableList<Line> {
        if (states[order] != OrderState.OPEN) throw TillException("That order isn't open.")
        return orders.getValue(order)
    }

    private fun due(order: String) = if (states[order] == OrderState.OPEN) {
        priced(order).second.first - checks[order].orEmpty().sumOf { checkPaid[it] ?: 0L }
    } else {
        0L
    }

    private fun money(minor: Long) = (if (minor < 0) "-$" else "$") + kotlin.math.abs(minor) / 100 + "." +
        (kotlin.math.abs(minor) % 100).toString().padStart(2, '0')
}
