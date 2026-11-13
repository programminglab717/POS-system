package dev.keel.android

import dev.keel.register.Chosen
import dev.keel.register.CashMovement
import dev.keel.register.DiscountAsked
import dev.keel.register.Till

/** A [Till] opened by [open] at its first call, on whichever thread makes it: the kernel's. */
class OpeningTill(open: () -> Till) : Till {
    private val opened = lazy(open)
    private val till get() = opened.value

    /** The till, if it was opened. */
    val openedTill: Till? get() = if (opened.isInitialized()) opened.value else null

    /** The till, opened now if it wasn't: call it on the kernel's thread. */
    fun open(): Till = till

    override fun team() = till.team()
    override fun signIn(member: String, pin: String) = till.signIn(member, pin)
    override fun approve(approver: String, pin: String) = till.approve(approver, pin)
    override fun approvers() = till.approvers()
    override fun signOut() = till.signOut()
    override fun menu() = till.menu()
    override fun item(variant: String) = till.item(variant)
    override fun startOrder() = till.startOrder()
    override fun addItem(order: String, variant: String, choices: List<Chosen>, quantity: Int) =
        till.addItem(order, variant, choices, quantity)
    override fun changeQuantity(order: String, line: String, quantity: Int) = till.changeQuantity(order, line, quantity)
    override fun removeLine(order: String, line: String) = till.removeLine(order, line)
    override fun applyDiscount(order: String, line: String?, discount: DiscountAsked, reason: String) =
        till.applyDiscount(order, line, discount, reason)
    override fun removeDiscount(order: String, discount: String) = till.removeDiscount(order, discount)
    override fun repriceLine(order: String, line: String, unitPriceMinor: Long, reason: String) =
        till.repriceLine(order, line, unitPriceMinor, reason)
    override fun setNotes(order: String, line: String, notes: String?) = till.setNotes(order, line, notes)
    override fun payCheckCash(order: String, check: String, partMinor: Long?, tenderedMinor: Long) =
        till.payCheckCash(order, check, partMinor, tenderedMinor)
    override fun splitEvenly(order: String, ways: Int) = till.splitEvenly(order, ways)
    override fun moveLine(order: String, line: String, to: String?) = till.moveLine(order, line, to)
    override fun recall(order: String) = till.recall(order)
    override fun drawer() = till.drawer()
    override fun openDrawer(floatMinor: Long) = till.openDrawer(floatMinor)
    override fun moveCash(movement: CashMovement, amountMinor: Long, reason: String) = till.moveCash(movement, amountMinor, reason)
    override fun noSale(reason: String) = till.noSale(reason)
    override fun closeDrawer(countedMinor: Long) = till.closeDrawer(countedMinor)
    override fun dayReport(z: Boolean) = till.dayReport(z)
    override fun printReport(z: Boolean) = till.printReport(z)
    override fun findOrders(code: String) = till.findOrders(code)
    override fun returnable(order: String) = till.returnable(order)
    override fun returnLines(order: String, lines: List<dev.keel.register.ReturnPick>, reason: String) =
        till.returnLines(order, lines, reason)
    override fun takeOver(order: String) = till.takeOver(order)
    override fun deviceId() = till.deviceId()
    override fun abandon(order: String) = till.abandon(order)
    override fun cashOffers(order: String) = till.cashOffers(order)
    override fun amount(minor: Long) = till.amount(minor)
    override fun payCash(order: String, tenderedMinor: Long) = till.payCash(order, tenderedMinor)
    override fun ticket(order: String) = till.ticket(order)
    override fun ordersToday() = till.ordersToday()
    override fun openOrders() = till.openOrders()
    override fun printJobs() = till.printJobs()
    override fun resolve(job: String, printed: Boolean) = till.resolve(job, printed)
    override fun reprint(order: String) = till.reprint(order)
    override fun eightySix(item: String, modifier: Boolean, untilPutBack: Boolean) =
        till.eightySix(item, modifier, untilPutBack)
    override fun putBack(item: String, modifier: Boolean) = till.putBack(item, modifier)
    override fun soldOut() = till.soldOut()
    override fun availabilityVersion() = openedTill?.availabilityVersion()
    override fun syncState() = openedTill?.syncState()

    override fun profileVersion() = openedTill?.profileVersion()
}
