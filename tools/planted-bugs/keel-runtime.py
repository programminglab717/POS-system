"""Planted bugs for keel-runtime: see run.py."""

# Integration tests of known answers, which --props leaves out.
KNOWN_ANSWERS = ["end_to_end"]

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
    # Each intent answers from inside its write.
    (
        "runtime: an intent's ticket is made after its write",
        "src/runtime.rs",
        """            append(w, order.cast(), &event, meta)?;
            view(w, order, &payments, profile, locale)
        })""",
        """            append(w, order.cast(), &event, meta)
        })?;
        self.ticket(order)""",
        # The property's orders always price: only an order that overflows shows it.
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
        "runtime: a negative tender is taken",
        "src/runtime.rs",
        """        if tendered.is_negative() {
            return Err(RuntimeError::NegativeTender);
        }
""",
        "",
    ),
    (
        "runtime: a tender is short only of what is due before cash rounding",
        "src/runtime.rs",
        """            if tendered.compare(cash.due)?.is_lt() {""",
        """            if tendered.compare(due)?.is_lt() {""",
    ),
    (
        "runtime: change ignores cash rounding",
        "src/runtime.rs",
        """            let change = tendered.checked_sub(cash.due)?;""",
        """            let change = tendered.checked_sub(due)?;""",
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
                let started =""",
        """            if !due.is_negative() {
                let started =""",
        # The demo sells nothing for nothing, and checkout refuses an order with no lines either
        # way.
        "unit",
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
        "runtime: a paid ticket lists its taxes by identifier",
        "src/ticket.rs",
        """    charges.sort_by_key(|charge| {
        rules.taxes.iter().position(|tax| tax.id == charge.tax).unwrap_or(usize::MAX)
    });
""",
        "",
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
    # Print jobs (ADR-0024).
    (
        "print: an unconfirmed job is tried again",
        "src/runtime.rs",
        "            Outcome::Unconfirmed(_) => {}",
        """            Outcome::Unconfirmed(_) => {
                let due = self.clock.now();
                self.store.write(|w| w.retry(&key, due))?;
            }""",
    ),
    (
        "print: a job running when the runtime opens is handed to the driver",
        "src/runtime.rs",
        "            .filter(|(_, queued)| queued.state == keel_store::EffectState::Pending)",
        "            .filter(|(_, queued)| queued.state != keel_store::EffectState::Done)",
    ),
    (
        "print: a job not sent waits as long each time",
        "src/print.rs",
        "    let wait = 2_u64.checked_shl(doublings).unwrap_or(60);",
        "    let wait = 2_u64.checked_shl(doublings.min(0)).unwrap_or(60);",
    ),
    (
        "print: a job not sent waits past a minute",
        "src/print.rs",
        "    Duration::from_secs(wait).min(MAX_WAIT)",
        "    Duration::from_secs(wait).min(MAX_WAIT.saturating_mul(2))",
        # A job fails to be sent six times in a row in few runs; the known answer counts them.
        "unit",
    ),
    (
        "print: jobs go to the driver two at a time",
        "src/runtime.rs",
        """        if self.sending.is_some() {
            return Ok(None);
        }
        let now = self.clock.now();""",
        """        let now = self.clock.now();""",
    ),
    (
        "print: any job's outcome is taken",
        "src/runtime.rs",
        "        if self.sending != Some(job) {\n            return Err(RuntimeError::UnknownJob(job));",
        "        if self.sending.is_none() {\n            return Err(RuntimeError::UnknownJob(job));",
    ),
    (
        "print: the job with the driver can be resolved",
        "src/runtime.rs",
        "        if queued.state != keel_store::EffectState::Running || self.sending == Some(job) {",
        "        if queued.state != keel_store::EffectState::Running {",
    ),
    (
        "print: a sale that owes nothing opens the drawer",
        "src/runtime.rs",
        """            if due.is_positive() {
                let kick = Doc::new(vec![Block::DrawerKick])?;""",
        """            if !due.is_negative() {
                let kick = Doc::new(vec![Block::DrawerKick])?;""",
    ),
    (
        "print: printing a job again leaves it unconfirmed",
        "src/runtime.rs",
        """                    w.fail(&key)?;
                    w.enqueue(&effect, now)?;""",
        """                    w.enqueue(&effect, now)?;""",
    ),
    (
        "print: a reprint printed again is marked twice",
        "src/runtime.rs",
        """        if is_reprint(doc) {
            return Ok(doc.clone());
        }""",
        """        if is_reprint(doc) && doc.blocks().is_empty() {
            return Ok(doc.clone());
        }""",
        # The property doesn't read documents; the known answers do.
        "unit",
    ),
    (
        "print: a receipt shows no change",
        "src/receipt.rs",
        """        if let Some(change) = captured.change() {
            blocks.push(Block::row(words.change, money(change)));""",
        """        if let Some(change) = captured.change().filter(|_| false) {
            blocks.push(Block::row(words.change, money(change)));""",
        # As above.
        "unit",
    ),
    # Found in review.
    (
        "print: jobs the clock left behind wait for it",
        "src/runtime.rs",
        """            if queued.due > now {
                self.store.write(|w| w.reschedule(&key, now))?;
            }""",
        "",
    ),
    (
        "print: a receipt's lines aren't capped by blocks",
        "src/receipt.rs",
        "            || lines.len().saturating_add(these.len()) > MAX_LINES_BLOCKS",
        "            || lines.len().saturating_add(these.len()) > usize::MAX",
        "unit",
    ),
    (
        "print: a receipt too big for the outbox fails the sale",
        "src/runtime.rs",
        "                .filter(|doc| doc.encode().len() <= keel_store::MAX_PAYLOAD)",
        "                .filter(|doc| !doc.blocks().is_empty())",
        "unit",
    ),
    (
        "print: a reprint is recognised only in English",
        "src/receipt.rs",
        "    [ENGLISH.reprint, SPANISH.reprint]",
        "    [ENGLISH.reprint]",
        "unit",
    ),
    (
        "print: jobs share process IDs",
        "src/print.rs",
        "    u64::from_be_bytes(tail).wrapping_add(u64::from(attempts))",
        "    u64::from(attempts).wrapping_add(u64::from(tail.len() > 99))",
        "unit",
    ),
]
