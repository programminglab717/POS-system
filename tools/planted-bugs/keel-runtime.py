"""Planted bugs for keel-runtime: see run.py."""

BUGS = [
    # Signing in, and what every event records.
    (
        "runtime: anyone may sign in",
        "src/runtime.rs",
        """        self.profile.member(member).ok_or(RuntimeError::UnknownMember(member))?;
        self.member = Some(member);""",
        """        self.member = Some(member);""",
        # The property signs in only a member of the demo's team.
        "unit",
    ),
    (
        "runtime: events record the local calendar date, not the business date",
        "src/runtime.rs",
        """            business_date: self.profile.business_day().business_date_of(now)?,""",
        """            business_date: keel_types::BusinessDayPolicy::new(
                self.profile.business_day().time_zone(),
                0,
                0,
            )?
            .business_date_of(now)?,""",
        # The property's clock reads 9 am, when the two agree; tickets don't show dates.
        "unit",
    ),
    (
        "runtime: events record no correlation",
        "src/runtime.rs",
        """        correlation: Some(meta.correlation),""",
        """        correlation: None,""",
        # Tickets don't show correlations.
        "unit",
    ),
    (
        "runtime: an order started belongs to no one",
        "src/runtime.rs",
        """            owner: self.member,""",
        """            owner: None,""",
        # Tickets don't show owners.
        "unit",
    ),
    # Abandoning.
    (
        "runtime: an order is abandoned with its lines still on it",
        "src/runtime.rs",
        """            for line in live {
                let event = loaded.decide(location, device, OrderCommand::RemoveLine(line))?;
                append(w, order.cast(), &event, &meta)?;
                loaded = w.load(Order::new(order))?;
            }""",
        """            let _ = live;""",
    ),
    # Paying cash.
    (
        "runtime: a tender is short only of what is due before cash rounding",
        "src/runtime.rs",
        """                if tendered.compare(cash.due)?.is_lt() {""",
        """                if tendered.compare(due)?.is_lt() {""",
    ),
    (
        "runtime: change ignores cash rounding",
        "src/runtime.rs",
        """                change = tendered.checked_sub(cash.due)?;""",
        """                change = tendered.checked_sub(due)?;""",
    ),
    (
        "runtime: a cash payment captures what is due after rounding",
        "src/runtime.rs",
        """                let captured = PaymentCaptured {
                    amount: due,""",
        """                let captured = PaymentCaptured {
                    amount: cash.due,""",
    ),
    (
        "runtime: a cash payment records no rounding",
        "src/runtime.rs",
        """                        rounding: (!cash.rounding.is_zero()).then_some(cash.rounding),""",
        """                        rounding: None,""",
        # Caught when cash rounds down: the cash tendered is then less than the payment's amount.
    ),
    (
        "runtime: an order with nothing due starts a payment",
        "src/runtime.rs",
        """            if due.is_positive() {
                let cash = cash_due(profile, due)?;""",
        """            if !due.is_negative() {
                let cash = cash_due(profile, due)?;""",
        # Caught when an order with no lines is tendered less than nothing: refused as short,
        # not by checkout.
    ),
    # Tickets.
    (
        "runtime: a closed ticket lists its lines by identifier",
        "src/ticket.rs",
        """    let lines = order
        .lines()
        .iter()
        .filter_map(|line| {
            let charge = closed.lines.iter().find(|charge| charge.line == line.id())?;
            Some(ticket_line(line, charge.gross, locale))
        })
        .collect();""",
        """    let lines = closed
        .lines
        .iter()
        .filter_map(|charge| Some(ticket_line(order.line(charge.line)?, charge.gross, locale)))
        .collect();""",
    ),
    (
        "runtime: a ticket's cash due ignores cash rounding",
        "src/ticket.rs",
        """    let cash = if due.is_positive() { cash_due(profile, due)?.due } else { due };""",
        """    let cash = due;""",
    ),
    (
        "runtime: a ticket lists taxes that taxed nothing",
        "src/ticket.rs",
        """        .filter(|tax| tax.taxable.is_positive())
        .map(|tax| TicketTax {""",
        """        .map(|tax| TicketTax {""",
    ),
    (
        "runtime: a ticket lists the lines taken off",
        "src/ticket.rs",
        """    let lines = order
        .live_lines()
        .zip(&totals.lines)""",
        """    let lines = order
        .lines()
        .iter()
        .zip(&totals.lines)""",
    ),
    (
        "runtime: a modifier chosen under another shows at its parent's depth",
        "src/views.rs",
        """        ticket_modifiers(&chosen.modifiers, depth.saturating_add(1), locale, out);""",
        """        ticket_modifiers(&chosen.modifiers, depth, locale, out);""",
    ),
    (
        "runtime: open orders include those closed",
        "src/runtime.rs",
        """            .orders(OrderState::Active)?
            .into_iter()""",
        """            .orders(OrderState::Active)?
            .into_iter()
            .chain(self.store.orders(OrderState::Closed)?)""",
    ),
    # The menu and the item view.
    (
        "runtime: the menu says modifiers must be chosen wherever they are offered",
        "src/runtime.rs",
        """                            required: groups().any(|group| group.min > 0),""",
        """                            required: groups().next().is_some(),""",
        # The property drives intents, not the menu.
        "unit",
    ),
    (
        "runtime: the menu names a variant by its item alone",
        "src/runtime.rs",
        """                            name: profile.variant_name(variant)?.clone(),""",
        """                            name: item.name.clone(),""",
        "unit",
    ),
    (
        "runtime: the item view leaves out the groups under a modifier",
        "src/views.rs",
        """                    groups: group_views(profile, &modifier.groups, locale),""",
        """                    groups: Vec::new(),""",
        "unit",
    ),
]
