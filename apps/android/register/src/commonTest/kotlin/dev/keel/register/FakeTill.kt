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

    private class Line(val id: String, val variant: String, var count: Int)

    val latte = Item(
        "latte", "Latte", "$4.50",
        listOf(
            Group("milk", null, "Milk", 1, 1, 0, false, listOf(Modifier("whole", "Whole", "$0.00"), Modifier("oat", "Oat", "$0.75"))),
            Group("shots", null, "Extra shots", 0, 3, 0, true, listOf(Modifier("shot", "Shot", "$0.95"))),
            Group("syrup", null, "Syrup", 0, 2, 0, false, listOf(Modifier("vanilla", "Vanilla", "$0.60"), Modifier("caramel", "Caramel", "$0.60"))),
            Group("strength", "vanilla", "Vanilla strength", 0, 1, 0, false, listOf(Modifier("double", "Double", "$0.30"))),
        ),
    )

    override fun team() = listOf(Member("alex", "Alex"), Member("sam", "Sam"))

    override fun signIn(member: String) {
        if (team().none { it.id == member }) throw TillException("no such member")
        signedIn = member
    }

    override fun signOut() {
        signedIn = null
    }

    override fun menu() = listOf(
        MenuPage("Drinks", listOf(MenuButton("latte", "Latte", "$4.50", choices = true, required = true), MenuButton("tea", "Tea", "$3.00"))),
        MenuPage("Food", listOf(MenuButton("scone", "Scone", "$2.75"))),
    )

    override fun item(variant: String): Item = if (variant == "latte") latte else throw TillException("no choices")

    override fun startOrder(): Ticket {
        check(signedIn != null) { "no one signed in" }
        val order = "order${orders.size + 1}"
        orders[order] = mutableListOf()
        states[order] = OrderState.OPEN
        return ticket(order)
    }

    override fun addItem(order: String, variant: String, choices: List<Chosen>, quantity: Int): Ticket {
        prices[variant] ?: throw TillException("not on the menu")
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

    override fun abandon(order: String): Ticket {
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
        states[order] = OrderState.CLOSED
        jobs += PrintJob("drawer:$order", JobKind.DRAWER, JobState.QUEUED, 0)
        jobs += PrintJob("receipt:$order", JobKind.RECEIPT, JobState.QUEUED, 0)
        return Paid(money(tenderedMinor - due), ticket(order))
    }

    override fun ticket(order: String): Ticket {
        val due = due(order)
        return Ticket(
            order = order,
            lines = orders.getValue(order).map { line ->
                TicketLine(line.id, line.variant, line.count.toString(), money(prices.getValue(line.variant) * line.count), line.count)
            },
            total = money(due),
            cashDueMinor = due,
            cashDue = money(due),
            state = states.getValue(order),
        )
    }

    override fun ordersToday() = orders.keys.map(::ticket)

    override fun printJobs() = jobs.toList()

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

    private fun due(order: String) =
        if (states[order] == OrderState.OPEN) orders.getValue(order).sumOf { prices.getValue(it.variant) * it.count } else 0L

    private fun money(minor: Long) = (if (minor < 0) "-$" else "$") + kotlin.math.abs(minor) / 100 + "." +
        (kotlin.math.abs(minor) % 100).toString().padStart(2, '0')
}
