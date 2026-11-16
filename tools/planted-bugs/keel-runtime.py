"""Planted bugs for keel-runtime: see run.py."""

# Integration tests of known answers, which --props leaves out.
KNOWN_ANSWERS = ["end_to_end", "publication", "sync"]

# The cell's ingest test runs a location with its cloud end to end, through the runtime: the
# durable peer's naming is seen there alone.
ALSO = ["keel-cloud"]

BUGS = [
    # Signing in, and what every event records.
    (
        "runtime: anyone may sign in",
        "src/staff.rs",
        """        if self.profile.member(member).is_none() {
            return Err(RuntimeError::UnknownMember(member));
        }""",
        "",
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
        """                append(w, order.cast(), &event, meta)?;
            }
            view(w, order, &payments, profile, locale)
        })""",
        """                append(w, order.cast(), &event, meta)?;
            }
            Ok::<(), RuntimeError>(())
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
        "                let captured = in_cash(applied, tendered, cash.rounding);",
        "                let captured = in_cash(cash.due, tendered, cash.rounding);",
    ),
    (
        "runtime: a cash payment records no rounding",
        "src/runtime.rs",
        "    let rounding = (!rounding.is_zero()).then_some(rounding);\n    let cash = Some(CashTendered { tendered, rounding });",
        "    let rounding = None::<Money>.filter(|_| !rounding.is_zero());\n    let cash = Some(CashTendered { tendered, rounding });",
        # Caught when cash rounds down: the cash tendered is then less than the payment's amount.
    ),
    (
        "runtime: an order with nothing due starts a payment",
        "src/runtime.rs",
        "            if applied.is_positive() || part.is_some() {",
        "            if !applied.is_negative() || part.is_some() {",
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
            let (_, gross, taken) =
                charges.lines.iter().find(|(known, _, _)| *known == line.id())?;""",
        """    let lines = charges
        .lines
        .iter()
        .filter_map(|(id, gross, taken)| {
            let line = order.line(*id)?;""",
    ),
    (
        "runtime: a paid ticket lists its taxes by identifier",
        "src/ticket.rs",
        """    taxes.sort_by_key(|(tax, _)| {
        rules.taxes.iter().position(|known| known.id == *tax).unwrap_or(usize::MAX)
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
        """            .filter(|tax| tax.taxable.is_positive())
            .map(|tax| (tax.id, tax.tax))""",
        """            .map(|tax| (tax.id, tax.tax))""",
    ),
    (
        "runtime: a ticket lists the lines taken off",
        "src/ticket.rs",
        "    let on: Vec<Id<Line>> = order.lines_on(check.id()).map(Line::id).collect();",
        "    let on: Vec<Id<Line>> = order.lines().iter().map(Line::id).collect();",
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
        """                    groups: group_views(profile, &modifier.groups, locale, sold_out),""",
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
        // A kitchen job names its printer""",
        """        // A kitchen job names its printer""",
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
        """            let mut paid = payments.clone();
            if applied.is_positive() || part.is_some() {""",
        """            let mut paid = payments.clone();
            if applied.is_zero() {
                let kick = Doc::new(vec![Block::DrawerKick])?;
                w.enqueue(&print::effect(drawer, JobKind::Drawer, &kick, None)?, meta.now)?;
            }
            if applied.is_positive() || part.is_some() {""",
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
        "        .filter(|doc| doc.encode().len() <= keel_store::MAX_PAYLOAD)",
        "        .filter(|doc| !doc.blocks().is_empty())",
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
    # Replication (ADR-0026).
    (
        "sync: a device writes before its log is settled",
        "src/runtime.rs",
        "        if self.sync.as_ref().is_some_and(|sync| !sync.may_write(self.clock.now())) {",
        "        if self.sync.as_ref().is_some_and(|sync| !sync.may_write(self.clock.now()) && false) {",
        "unit",
    ),
    (
        "sync: an intent's events wait for the next round",
        "src/sync.rs",
        "            let events = self.store.log(device, pushed, PUSH)?;",
        "            let events = self.store.log(device, u64::MAX, PUSH)?;",
        "unit",
    ),
    (
        "sync: a roster naming another key for the device is taken",
        "src/sync.rs",
        "        if roster.data().location != self.profile.data().location || me.key != self.key {",
        "        if roster.data().location != self.profile.data().location {",
        "unit",
    ),
    # Found in the slice's review.
    (
        "sync: a tick a set-back clock put off is waited for",
        "src/sync.rs",
        "            .is_some_and(|at| at <= now || at.as_millis().saturating_sub(now.as_millis()) > longest)",
        "            .is_some_and(|at| at <= now && longest > 0)",
        "unit",
    ),
    (
        "sync: a device that hears no one never writes",
        "src/sync.rs",
        "        self.replicator.settled() || waited >= island || waited < 0",
        "        self.replicator.settled() || waited < 0",
        "unit",
    ),
    (
        "sync: a numbering that fails isn't tried again",
        "../keel-sync/src/replicator.rs",
        "            if self.serving() && (!was_serving || self.owed) {",
        "            if self.serving() && !was_serving {",
        "unit",
    ),
    (
        "sync: a hub the roster demotes goes on serving",
        "../keel-sync/src/replicator.rs",
        "        self.settled && !self.forked() && self.is_hub() && self.roles.hub.is_some()",
        "        self.settled && !self.forked() && self.is_hub()",
        "unit",
    ),
    (
        "runtime: today's orders are every day's",
        "src/runtime.rs",
        ".filter(|order| order.business_date == Some(today))",
        ".filter(|order| order.business_date.is_some())",
        "unit",
    ),
    (
        "runtime: today's orders leave out the closed",
        "src/runtime.rs",
        "[OrderState::Active, OrderState::Closed, OrderState::Voided, OrderState::Abandoned]",
        "[OrderState::Active, OrderState::Voided, OrderState::Abandoned]",
        "unit",
    ),
    (
        "runtime: a cash offer is the multiple at or below what is due",
        "src/ticket.rs",
        ".and_then(|unit| due.checked_div(unit)?.checked_add(1)?.checked_mul(unit));",
        ".and_then(|unit| due.checked_div(unit)?.checked_add(0)?.checked_mul(unit));",
        # The property test that catches it is a unit test, in src/tests.rs.
        "unit",
    ),
    (
        "runtime: one cash offer too many",
        "src/ticket.rs",
        "            && offers.len() < crate::CASH_OFFERS",
        "            && offers.len() <= crate::CASH_OFFERS",
        # The property test that catches it is a unit test, in src/tests.rs.
        "unit",
    ),
    (
        "runtime: an order that owes nothing is offered more",
        "src/ticket.rs",
        "    if due <= 0 {\n        return offers;",
        "    if due < 0 {\n        return offers;",
        # The property test that catches it is a unit test, in src/tests.rs.
        "unit",
    ),
    (
        "runtime: a cash offer may repeat",
        "src/ticket.rs",
        "            && !offers.contains(&above)\n",
        "",
        # The property test that catches it is a unit test, in src/tests.rs.
        "unit",
    ),
    (
        "runtime: a currency without decimals is offered single units",
        "src/ticket.rs",
        "    let major = if minor_units == 0 {\n        100\n    } else {",
        "    let major = if minor_units == 0 {\n        1\n    } else {",
        "unit",
    ),
    # The cloud (ADR-0028).
    (
        "runtime: a hub names no durable peer, so no device hears it is backed up",
        "src/sync.rs",
        "            .filter(|cloud| hub.is_some() && *cloud != device);",
        "            .filter(|_| false);",
    ),
    (
        "runtime: a device's backed-up mark is its own head",
        "src/sync.rs",
        "            backed_up: sync.replicator.durable().get(&self.device).copied().unwrap_or(0),",
        "            backed_up: self.store.head(self.device)?.seq(),",
    ),
    # Catalog publishing (ADR-0028).
    (
        "runtime: any device publishes",
        "src/publication.rs",
        "        if sync.roster.cloud().is_none_or(|cloud| cloud.device != self.device) {\n            return Err(RuntimeError::NotCloud);",
        "        if sync.roster.cloud().is_none() {\n            return Err(RuntimeError::NotCloud);",
    ),
    (
        "runtime: a publication of another location is taken",
        "src/publication.rs",
        "            .filter(|profile| profile.data().location == self.profile.data().location))",
        "            )",
    ),
    (
        "runtime: the earliest publication is taken, not the latest",
        "src/publication.rs",
        "        for (stream, published) in found.iter().rev() {",
        "        for (stream, published) in found.iter() {",
    ),
    (
        "runtime: a member the new team doesn't have stays signed in",
        "src/publication.rs",
        "            self.member = None;\n",
        "",
    ),
    (
        "runtime: a publication is looked for at no frame",
        "src/sync.rs",
        "        // look began; the frame itself is stored, and its answers go out.\n        let _ = self.take_publications();",
        "        // look began; the frame itself is stored, and its answers go out.",
    ),
    (
        "runtime: a device restarted opens with the provisioned profile",
        "src/sync.rs",
        "        // first frames go out.\n        let _ = self.take_publications();",
        "        // first frames go out.",
    ),
    (
        "runtime: a device that publishes doesn't take its own",
        "src/publication.rs",
        "        })?;\n        self.take_publications()?;",
        "        })?;",
    ),
    # Found in step 8's review.
    (
        "runtime: the cloud publishes before its log is settled",
        "src/publication.rs",
        "        if !sync.may_write(self.clock.now()) {",
        "        if !sync.may_write(self.clock.now()) && false {",
    ),
    (
        "runtime: a device takes no publication before it joins its network",
        "src/publication.rs",
        "        self.watch_cloud(roster.cloud().map(|cloud| cloud.device));\n        self.take_publications()",
        "        self.watch_cloud(roster.cloud().map(|cloud| cloud.device));\n        Ok(false)",
    ),
    (
        "runtime: another cloud's log is looked at from where the last one's was",
        "src/publication.rs",
        "            self.published = Watch { cloud, looked: 0 };",
        "            self.published = Watch { cloud, looked: self.published.looked };",
    ),
    # Staff, roles and approvals (ADR-0029).
    (
        "runtime: PIN entry is never locked",
        "src/staff.rs",
        "        if let Some(seconds) = self.pins.locked(now) {",
        "        if let Some(seconds) = self.pins.locked(now).filter(|_| false) {",
    ),
    (
        "runtime: a right PIN starts the count of wrong ones again, whoever's it is",
        "src/staff.rs",
        "        if self.profile.verifies_pin(member, pin) {\n            Ok(())",
        "        if self.profile.verifies_pin(member, pin) {\n            self.pins = PinGuard::default();\n            Ok(())",
    ),
    (
        "runtime: every lock is as short as the first",
        "src/staff.rs",
        "            let lock = FIRST_LOCK.saturating_mul(1 << doublings).min(LONGEST_LOCK);",
        "            let lock = FIRST_LOCK;",
    ),
    (
        "runtime: a clock set back makes a lock longer than the longest",
        "src/staff.rs",
        "        let left = left.min(millis(LONGEST_LOCK));",
        "",
    ),
    (
        "runtime: an approval never expires",
        "src/staff.rs",
        "        (pending.asked == *asked && (0..=millis(APPROVAL_LIFE)).contains(&age))",
        "        (pending.asked == *asked && (0..=i64::MAX).contains(&age))",
    ),
    (
        "runtime: an approval outlives a sign-in",
        "src/staff.rs",
        "        self.member = Some(member);\n        self.pending = None;",
        "        self.member = Some(member);",
    ),
    (
        "runtime: an approval is never spent",
        "src/staff.rs",
        "        if granted.is_some() {\n            self.pending = None;",
        "        if granted.is_some() {",
    ),
    (
        "runtime: anyone's approval approves anything",
        "src/staff.rs",
        "        if !self.profile.may_approve(approver, member, &request) {",
        "        if false {",
    ),
    (
        "runtime: the approved events don't name their approval",
        "src/staff.rs",
        "        meta.approval = Some(approval);\n",
        "        let _ = approval;\n",
    ),
    (
        "runtime: dropping an empty order needs an approval",
        "src/runtime.rs",
        "        let granted = if loaded.lines().is_empty() {",
        "        let granted = if false {",
    ),
    (
        "runtime: anyone reprints, signed in or not",
        "src/runtime.rs",
        "        let granted = self.gate(Request::of(Action::ReceiptReprint), Some(order.cast()))?;",
        "        let granted: Option<Granted> = None;",
    ),
    # Found in step 1's review.
    (
        "runtime: quiet never starts the count of wrong PINs again",
        "src/staff.rs",
        "        if quiet {\n            self.wrong = 0;",
        "        if quiet && false {\n            self.wrong = 0;",
    ),
    (
        "runtime: an approval covers any request of the one who asked",
        "src/staff.rs",
        "        (pending.asked == *asked && (0..=millis(APPROVAL_LIFE)).contains(&age))",
        "        (pending.asked.actor == asked.actor && (0..=millis(APPROVAL_LIFE)).contains(&age))",
    ),
    (
        "runtime: an approver is taken before they are known to be one",
        "src/staff.rs",
        "        if !self.profile.may_approve(approver, actor, &asked.request) {",
        "        if false {",
    ),
    (
        "runtime: an order emptied line by line is dropped with no approval",
        "src/runtime.rs",
        "        let granted = if loaded.lines().is_empty() {",
        "        let granted = if loaded.live_lines().next().is_none() {",
    ),
    (
        "runtime: fewer of a line needs no approval",
        "src/runtime.rs",
        """        let request = if fewer {
            Some(Request::of(Action::LineRemove))""",
        """        let request = if false {
            Some(Request::of(Action::LineRemove))""",
    ),
    (
        "runtime: an approval is asked for a reprint that can't be",
        "src/runtime.rs",
        "        // Someone is signed in first: whether a receipt exists is for them to learn.\n        self.member.ok_or(RuntimeError::NotSignedIn)?;",
        "        self.gate(Request::of(Action::ReceiptReprint), Some(order.cast()))?;",
    ),
    (
        "runtime: anyone but the one who asked is an approver",
        "src/staff.rs",
        "            .filter(|&member| self.profile.may_approve(member, asked.actor, &asked.request))",
        "            .filter(|&member| member != asked.actor)",
    ),
    (
        "runtime: a member whose PIN or role changed stays signed in",
        "src/publication.rs",
        "                (Some(before), Some(after)) => before.role != after.role || before.pin != after.pin,",
        "                (Some(_), Some(_)) => false,",
    ),
    # Discounts (ADR-0030): the gate.
    (
        "discount: the gate is asked for no amount",
        "src/runtime.rs",
        """            amount: Some(effect.total),
            percent: Some(effect.percent),
        };
        let granted""",
        """            amount: None,
            percent: Some(effect.percent),
        };
        let granted""",
    ),
    (
        "discount: the gate is asked for no percentage",
        "src/runtime.rs",
        """            percent: Some(effect.percent),
        };
        let granted""",
        """            percent: None,
        };
        let granted""",
    ),
    (
        "discount: a discount isn't gated",
        "src/runtime.rs",
        """            percent: Some(effect.percent),
        };
        let granted = self.gate(request, Some(order.cast()))?;""",
        """            percent: Some(effect.percent),
        };
        let granted: Option<Granted> = None;
        let _ = request;""",
    ),
    (
        "discount: a discount the order refuses asks for an approval first",
        "src/runtime.rs",
        """        let command = OrderCommand::ApplyDiscount(applied.clone());
        loaded.decide(self.location(), self.device, command.clone())?;""",
        """        let command = OrderCommand::ApplyDiscount(applied.clone());""",
    ),
    (
        "discount: a receipt leaves out a line's discounts",
        "src/receipt.rs",
        "        for discount in &line.discounts {",
        "        for discount in line.discounts.iter().take(0) {",
    ),
    (
        "discount: a receipt leaves out the whole order's discounts",
        "src/receipt.rs",
        "    for discount in &ticket.discounts {",
        "    for discount in ticket.discounts.iter().take(0) {",
    ),
    (
        "reprice: the gate is asked for the unit difference, not the line's",
        "src/runtime.rs",
        "        let difference = at(price)?.checked_sub(at(rung)?)?;",
        "        let difference = price.checked_sub(rung)?;",
    ),
    (
        "reprice: a price set by hand isn't gated",
        "src/runtime.rs",
        """            Request { action: Action::PriceOverride, amount: Some(amount), percent: None };
        let granted = self.gate(request, Some(order.cast()))?;""",
        """            Request { action: Action::PriceOverride, amount: Some(amount), percent: None };
        let _ = request;
        let granted: Option<Granted> = None;""",
    ),
    (
        "reprice: a price the order refuses asks for an approval first",
        "src/runtime.rs",
        """        let command = OrderCommand::RepriceLine(repriced);
        // The order's own rules come before an approval is asked for one it would refuse.
        loaded.decide(self.location(), self.device, command.clone())?;""",
        """        let command = OrderCommand::RepriceLine(repriced);""",
    ),
    (
        "notes: a receipt leaves out a line's notes",
        "src/receipt.rs",
        "        if let Some(notes) = &line.notes {",
        "        if let Some(notes) = line.notes.as_ref().filter(|_| false) {",
    ),
    # Split checks and partial payments (ADR-0030).
    (
        "split: an order's ticket leaves out its checks' taxes",
        "src/ticket.rs",
        "        for (tax, amount) in other.taxes {",
        "        for (tax, amount) in other.taxes.into_iter().take(0) {",
    ),
    (
        "split: an order's ticket counts one check's amounts",
        "src/ticket.rs",
        "        self.total = self.total.checked_add(other.total)?;",
        "        self.total = other.total;",
    ),
    (
        "split: a part pays all the check owes",
        "src/runtime.rs",
        "                Some(part) => (part, Cash { due: part, rounding: Money::zero(currency) }),",
        "                Some(_) => (due, cash_due(profile, due)?),",
    ),
    (
        "split: the order closes with its first check",
        "src/runtime.rs",
        "            .all(|check| !check.is_open() || order.lines_on(check.id()).next().is_none())",
        "            .any(|check| !check.is_open() || order.lines_on(check.id()).next().is_none())",
    ),
    (
        "split: no checks are opened for it",
        "src/runtime.rs",
        "        let more = usize::from(ways).saturating_sub(open.len());",
        "        let more = 0_usize;",
    ),
    (
        "split: a check's receipt shows all the order's payments",
        "src/runtime.rs",
        "        .filter(|payment| payment.info().is_some_and(|info| info.check == check))",
        "        .filter(|payment| payment.info().is_some())",
    ),
    (
        "split: a check's receipt doesn't say which check",
        "src/runtime.rs",
        "        check: number,",
        "        check: None,",
    ),
    (
        "split: a part closes the check",
        "src/runtime.rs",
        "    if checkout.balance(check)?.due.is_positive() {",
        "    if false {",
    ),
    # Park and recall (ADR-0030).
    (
        "recall: an order asked for already is asked for again",
        "src/runtime.rs",
        "        if ours || asked {",
        "        if ours {",
    ),
    (
        "recall: an order owned here is asked for",
        "src/runtime.rs",
        "        let ours = loaded.ownership().is_some_and(|ownership| ownership.device == self.device);",
        "        let ours = false;",
    ),
    (
        "recall: taking an order over isn't gated",
        "src/runtime.rs",
        """        let granted = self.gate(Request::of(Action::OrderVoid), Some(order.cast()))?;
        let meta = self.meta()?;
        let stream = self.approval_stream(granted.as_ref())?;
        let ticket = self.decide_approved(order, command, &meta, granted.as_ref(), stream)?;""",
        """        let granted: Option<Granted> = None;
        let meta = self.meta()?;
        let stream = self.approval_stream(granted.as_ref())?;
        let ticket = self.decide_approved(order, command, &meta, granted.as_ref(), stream)?;""",
    ),
    (
        "recall: a ticket names no owner",
        "src/ticket.rs",
        "        owner: order.ownership().map(|ownership| ownership.device),",
        "        owner: None,",
    ),
    # Found in step 2's review (ADR-0030).
    (
        "review: an amount larger than its line is accepted",
        "src/runtime.rs",
        "            && effect.taken != amount",
        "            && false",
    ),
    (
        "review: a partly paid check takes a discount",
        "src/runtime.rs",
        "        self.check_unpaid(&loaded, touched(&loaded, applied.line))?;",
        "",
    ),
    (
        "review: a discount is asked for as it takes alone",
        "src/runtime.rs",
        """            amount: Some(effect.total),
            percent: Some(effect.percent),
        };
        let granted = self.gate(request, Some(order.cast()))?;""",
        """            amount: Some(effect.taken),
            percent: Some(effect.percent),
        };
        let granted = self.gate(request, Some(order.cast()))?;""",
    ),
    (
        "review: a price is asked for from the last one set",
        "src/runtime.rs",
        "        let amount = self.override_amount(unit_price, now.item().unit_price, now.quantity())?;",
        "        let amount = self.override_amount(unit_price, now.unit_price(), now.quantity())?;",
    ),
    (
        "review: more of a discounted line isn't asked for",
        "src/runtime.rs",
        "        } else if more && (!now.discounts().is_empty() || !loaded.discounts().is_empty()) {",
        "        } else if false {",
    ),
    (
        "review: more of a line priced by hand isn't asked for",
        "src/runtime.rs",
        "        } else if more && now.repriced().is_some() {",
        "        } else if false {",
    ),
    (
        "review: an emptied split order stays open",
        "src/runtime.rs",
        """            if finished(&loaded) {
                let event = loaded.decide(location, device, OrderCommand::Close)?;
                append(w, order.cast(), &event, meta)?;""",
        """            if false {
                let event = loaded.decide(location, device, OrderCommand::Close)?;
                append(w, order.cast(), &event, meta)?;""",
    ),
    (
        "review: pay_cash pays the main check",
        "src/runtime.rs",
        "            [only] => Ok(*only),",
        "            [_] => Ok(order.cast()),",
    ),
    (
        "review: a ticket lists no lone check but the main",
        "src/ticket.rs",
        "    if checks.len() > 1 || checks.iter().any(|known| known.check != main) {",
        "    if checks.len() > 1 || main != main {",
    ),
    (
        "review: a split order's reprint is its last check's alone",
        "src/runtime.rs",
        "            originals.retain(|(_, known)| *known != cause);",
        "            originals.clear();",
    ),
    (
        "review: a smaller line on a partly paid check is allowed",
        "src/runtime.rs",
        """        if fewer {
            self.check_unpaid(&loaded, now.allocation().iter().map(|share| share.check))?;
        }""",
        "",
    ),
    # Drawers and day reports (ADR-0031).
    (
        "drawer: cash goes into no drawer",
        "src/runtime.rs",
        "                if let (Some(open), PaymentEvent::Captured(captured)) = (drawer_session, &event)",
        "                if let (Some(open), PaymentEvent::Captured(captured)) = (None::<Id<Drawer>>, &event)",
    ),
    (
        "drawer: the drawer counts the amount, not what the customer paid",
        "src/runtime.rs",
        "                    && let Some(amount) = captured.received().filter(|amount| amount.is_positive())",
        "                    && let Some(amount) = Some(captured.amount).filter(|amount| amount.is_positive())",
    ),
    (
        "drawer: a second drawer opens",
        "src/drawer.rs",
        """        if self.open_drawer_id()?.is_some() {
            return Err(RuntimeError::DrawerOpen);
        }""",
        "",
    ),
    (
        "drawer: a paid-out isn't gated",
        "src/drawer.rs",
        "        let granted = if kind == Movement::PaidOut {",
        "        let granted = if false {",
    ),
    (
        "drawer: a no-sale isn't gated",
        "src/drawer.rs",
        "        let granted = self.gate(Request::of(Action::DrawerNoSale), Some(open.cast()))?;",
        "        let granted = None;",
    ),
    (
        "drawer: a variance is asked for as nothing",
        "src/drawer.rs",
        "        let granted = if variance.is_zero() {",
        "        let granted = if true {",
    ),
    (
        "drawer: a short close is asked for as its sign",
        "src/drawer.rs",
        "            let size = if variance.is_negative() { variance.checked_neg()? } else { variance };",
        "            let size = variance;",
    ),
    (
        "drawer: the open drawer shows what it took",
        "src/views.rs",
        "            float: amount(info.float),\n            taken: None,",
        "            float: amount(info.float),\n            taken: Some(amount(sums.taken)),",
    ),
    (
        "report: a Z prints with a drawer open",
        "src/report.rs",
        "        if kind == ReportKind::Z",
        "        if kind == ReportKind::Z && false",
    ),
    (
        "report: discounts on the whole order aren't counted",
        "src/report.rs",
        """            for discount in &ticket.discounts {
                discounts = discounts.checked_add(discount.amount.money)?;
            }""",
        "",
    ),
    (
        "report: cash a drawer counted is reported as with no drawer",
        "src/report.rs",
        "            if info.tender == Tender::Cash && !self.store.in_drawer(payment.id)? {",
        "            if info.tender == Tender::Cash {",
    ),
    (
        "report: a dropped order with lines isn't voided",
        "src/report.rs",
        "            if order.state == OrderState::Voided || !loaded.lines().is_empty() {",
        "            if order.state == OrderState::Voided {",
    ),
    (
        "report: another day's orders are counted",
        "src/report.rs",
        "            if order.business_date != Some(date)\n                ||",
        "            if false\n                ||",
    ),
    (
        "report: a Z passes with the day before's drawer open",
        "src/report.rs",
        "        if kind == ReportKind::Z && self.open_drawer_id()?.is_some() {",
        "        if kind == ReportKind::Z && self.store.drawers_on(date)?.iter().any(|drawer| drawer.state == keel_store::DrawerState::Open) {",
    ),
    (
        "report: payments toward an order still open aren't counted",
        "src/report.rs",
        "            let loaded = self.store.load(Payment::new(payment.id))?;",
        "            if self.store.order(info.order)?.is_none_or(|order| order.state != OrderState::Closed) { continue; }\n            let loaded = self.store.load(Payment::new(payment.id))?;",
    ),
    (
        "report: cash rounding is left out of the tenders",
        "src/report.rs",
        "            let Some(amount) = loaded.captured().and_then(PaymentCaptured::received) else {",
        "            let Some(amount) = loaded.captured().map(|captured| captured.amount) else {",
    ),
    (
        "drawer: the register works a drawer of another currency",
        "src/drawer.rs",
        "                    info.location == location && info.float.currency() == currency",
        "                    info.location == location",
    ),
    # Returns at the register (ADR-0032).
    (
        "return: a refund isn't gated",
        "src/returns.rs",
        "            self.gate(request, Some(order.cast()))?",
        "            {\n                let _ = request;\n                None\n            }",
    ),
    (
        "return: a refund is gated for no amount",
        "src/returns.rs",
        "Request { action: Action::RefundIssue, amount: Some(owed), percent: None };",
        "Request { action: Action::RefundIssue, amount: None, percent: None };",
    ),
    (
        "return: the cash handed back isn't rounded",
        "src/returns.rs",
        "    let rounded = cash_due(profile, cash_owed)?.due;",
        "    let rounded = cash_owed;",
    ),
    (
        "return: cash is rounded return by return",
        "src/returns.rs",
        "    let handed = most.checked_sub(sums.handed)?;",
        "    let handed = cash_due(profile, total)?.due;\n    let _ = most;",
    ),
    (
        "return: a return split in parts passes the role's limit",
        "src/returns.rs",
        "Request { action: Action::RefundIssue, amount: Some(owed), percent: None };",
        "Request { action: Action::RefundIssue, amount: Some(total), percent: None };",
    ),
    (
        "return: the refund isn't taken out of the drawer",
        "src/returns.rs",
        "            if let Some(open) = drawer_session.filter(|_| handed.is_positive()) {",
        "            if let Some(open) = drawer_session.filter(|_| false) {",
    ),
    (
        "return: the drawer doesn't open for a refund",
        "src/returns.rs",
        "            if handed.is_positive() {",
        "            if false {",
    ),
    (
        "return: what is left counts nothing returned",
        "src/returns.rs",
        "            let left = line.quantity().checked_sub(returned)?;",
        "            let left = line.quantity();",
    ),
    (
        "return: open orders are found from a receipt",
        "src/returns.rs",
        "            .orders(OrderState::Closed)?",
        "            .orders(OrderState::Active)?",
    ),
    (
        "report: a refund's amount, not the cash handed back",
        "src/report.rs",
        "            let Some(handed) = completed.handed().filter(|handed| handed.currency() == currency)",
        "            let Some(handed) = Some(completed.amount).filter(|handed| handed.currency() == currency)",
    ),
    (
        "report: a refund a drawer paid is reported as with no drawer",
        "src/report.rs",
        "                && !self.store.refund_in_drawer(refund.id)?\n",
        "",
    ),
    (
        "return: a part of a receipt's code finds an order",
        "src/returns.rs",
        "        if code.chars().count() != RECEIPT_CODE {",
        "        if code.is_empty() {",
    ),
    (
        "availability: ringing doesn't look at what is sold out",
        "src/runtime.rs",
        "        self.check_on_sale(variant, choices)?;\n",
        "",
    ),
    (
        "availability: a modifier chosen isn't looked at",
        "src/availability.rs",
        "            modifiers.push(choice.modifier);\n",
        "",
    ),
    (
        "availability: choices under a choice aren't looked at",
        "src/availability.rs",
        "            left.extend(choice.choices.iter());\n",
        "",
    ),
    (
        "availability: an 86 for the day lasts until put back",
        "src/availability.rs",
        "                Some(day.window(day.business_date_of(meta.now)?)?.1)",
        "                None",
    ),
    (
        "availability: an 86 for the day ends when it began",
        "src/availability.rs",
        "                Some(day.window(day.business_date_of(meta.now)?)?.1)",
        "                Some(day.window(day.business_date_of(meta.now)?)?.0)",
    ),
    (
        "availability: an 86 isn't gated",
        "src/availability.rs",
        "        let granted = self.gate(Request::of(Action::ItemAvailability), Some(id.cast()))?;",
        "        let granted = None;",
    ),
    (
        "availability: an approval isn't spent",
        "src/availability.rs",
        "        self.spend(granted.as_ref());\n        Ok(())\n    }\n\n    /// Refuses",
        "        Ok(())\n    }\n\n    /// Refuses",
    ),
    (
        "availability: an item the catalog lacks is 86'd",
        "src/availability.rs",
        "        if !known {",
        "        if false && !known {",
    ),
    (
        "availability: the menu shows nothing sold out",
        "src/runtime.rs",
        "                            sold_out: sold_out\n                                .get(&variant.cast())\n                                .copied()\n                                .map(crate::views::SoldOutUntil),",
        "                            sold_out: None.filter(|_: &crate::views::SoldOutUntil| sold_out.is_empty()),",
    ),
    (
        "availability: the sheet shows nothing sold out",
        "src/views.rs",
        "                    sold_out: sold_out.get(&modifier.id.cast()).copied().map(SoldOutUntil),",
        "                    sold_out: None,",
    ),
    (
        "availability: a modifier's 86 is listed under the variant's identifier",
        "src/availability.rs",
        "                (ItemRef::Modifier(modifier), found.name.clone())",
        "                (ItemRef::Variant(modifier.cast()), found.name.clone())",
    ),
    (
        "availability: more of a line sold out since it was rung",
        "src/runtime.rs",
        "        if more {\n            self.check_line_on_sale(now)?;\n        }\n",
        "",
    ),
    (
        "availability: a line's modifiers aren't looked at when it grows",
        "src/availability.rs",
        "            modifiers.push(chosen.modifier);\n",
        "",
    ),
    (
        "availability: an 86 for the day ends at the calendar day's cutoff",
        "src/availability.rs",
        "                Some(day.window(day.business_date_of(meta.now)?)?.1)",
        "                Some(day.window(day.local_time(meta.now)?.date)?.1)",
    ),
    # The kitchen (ADR-0034).
    (
        "kitchen: a modifier left out adds its allergens",
        "src/kitchen.rs",
        "        if modifier.prefix == Prefix::No {\n            continue;\n        }\n",
        "",
    ),
    (
        "kitchen: a component's quantity is its line's",
        "src/kitchen.rs",
        "        .checked_mul(i64::from(chosen.quantity.get()))",
        "        .checked_mul(1)",
    ),
    (
        "kitchen: fired lines are fired again",
        "src/kitchen.rs",
        "            .filter(|line| *line.status() == LineStatus::Pending)",
        "            .filter(|line| line.is_live())",
    ),
    (
        "kitchen: paying a check fires the other checks' lines",
        "src/kitchen.rs",
        "            .filter(|line| on_check.as_ref().is_none_or(|on| on.contains(&line.id())))",
        "            .filter(|line| on_check.as_ref().is_none_or(|on| on.contains(&line.id()) || !on.is_empty()))",
    ),
    (
        "kitchen: a station no screen shows prints nothing",
        "src/kitchen.rs",
        "                (false, Some(printer)) => {",
        "                (false, Some(printer)) if false => {",
    ),
    (
        "kitchen: sending fires nothing",
        "src/kitchen.rs",
        "                write_firing(w, firing, location, device, &meta)?;",
        "                let _ = firing;",
    ),
    (
        "kitchen: paying fires nothing",
        "src/runtime.rs",
        "                crate::kitchen::write_firing(w, firing, location, device, &meta)?;",
        "                let _ = firing;",
    ),
    (
        "kitchen: a screen records no showing",
        "src/kitchen.rs",
        "            .filter(|summary| summary.shown_at.is_none() && summary.rerouted.is_none())",
        "            .filter(|summary| summary.shown_at.is_some() && summary.rerouted.is_none())",
    ),
    (
        "kitchen: a voided line isn't struck through",
        "src/kitchen.rs",
        "                    void: order_void || line_void,",
        "                    void: order_void,",
    ),
    (
        "kitchen: a voided order isn't struck through",
        "src/kitchen.rs",
        "                    void: order_void || line_void,",
        "                    void: line_void,",
    ),
    (
        "kitchen: a ticket is late as soon as late-ish",
        "src/kitchen.rs",
        "        let aging = if age >= u64::from(kitchen.aging.late.get()) {",
        "        let aging = if age >= u64::from(kitchen.aging.late_ish.get()) {",
    ),
    (
        "kitchen: expo sees a line made when any station has bumped it",
        "src/kitchen.rs",
        "                *ready = *ready && ticket.is_bumped();",
        "                *ready = *ready || ticket.is_bumped();",
    ),
    (
        "kitchen: expo's own ticket counts as making",
        "src/kitchen.rs",
        "            if kitchen.station(sent.station).is_none_or(|station| station.expo) {\n                continue;\n            }\n",
        "",
    ),
    (
        "kitchen: a ticket for a printer-only station is rerouted",
        "src/kitchen.rs",
        "                    .is_some_and(|station| station.screen)",
        "                    .is_some()",
    ),
    (
        "kitchen: a reroute ignores the station's printer",
        "src/kitchen.rs",
        "                (Some(printer), _) => (RerouteTo::StationPrinter, Some(printer.clone())),",
        "                (Some(_), None) => (RerouteTo::Nowhere, None),\n                (Some(_), Some(printer)) => (RerouteTo::FallbackPrinter, Some(printer.clone())),",
    ),
    (
        "kitchen: the print loop doesn't reroute",
        "src/runtime.rs",
        "        let _rerouted = self.reroute_overdue();\n",
        "",
    ),
    (
        "kitchen: a kitchen job waits for the register's own printer",
        "src/runtime.rs",
        "                    .is_some_and(|(_, kind)| kind == JobKind::Kitchen || own.is_some())",
        "                    .is_some_and(|_| own.is_some())",
    ),
    (
        "kitchen: a void slips every line of the ticket",
        "src/kitchen.rs",
        "                sent.items.iter().filter(|item| lines.contains(&item.line)).cloned().collect();",
        "                sent.items.to_vec();",
    ),
    (
        "kitchen: a void slips nowhere for a printer-only station",
        "src/kitchen.rs",
        "                _ if !station.screen => station.printer.clone(),",
        "",
    ),
    (
        "kitchen: dropping a sent order abandons it",
        "src/runtime.rs",
        "            if loaded.lines().iter().any(keel_domain::order::Line::was_fired) {",
        "            if false && loaded.lines().iter().any(keel_domain::order::Line::was_fired) {",
    ),
    (
        "kitchen: a long text isn't clipped",
        "src/kitchen.rs",
        "    if text.chars().count() <= MOST_CHARS {",
        "    if text.chars().count() <= MOST_CHARS * 10 {",
    ),
    (
        "kitchen: a reroute prints what was voided since",
        "src/kitchen.rs",
        "                _ if live.is_empty() => (RerouteTo::Nowhere, None),\n",
        "",
    ),
    (
        "kitchen: a device that doesn't own an order sends it",
        "src/kitchen.rs",
        "            && owner.device != self.device",
        "            && owner.device != owner.device",
    ),
    (
        "kitchen: a kitchen job waits for the register's printer's word",
        "src/runtime.rs",
        "            let printer = if to.is_some() { kitchen_printer } else { printer };",
        "            let printer = if to.is_some() { printer } else { kitchen_printer };",
    ),
    (
        "kitchen: lines no station makes aren't fired",
        "src/kitchen.rs",
        "    let loaded = w.load(Order::new(firing.order))?;",
        "    if firing.tickets.is_empty() {\n        return Ok(());\n    }\n    let loaded = w.load(Order::new(firing.order))?;",
    ),
    # Card payments (ADR-0035).
    (
        "card: a request started before is sent again",
        "src/card.rs",
        "CardRequest { key: key_of(&read.ask), ask: read.ask, status: started.attempts > 1 };",
        "CardRequest { key: key_of(&read.ask), ask: read.ask, status: started.attempts > 99 };",
    ),
    (
        "card: a request in doubt isn't asked about",
        "src/card.rs",
        "                EffectState::Running => self.store.write(|w| w.retry(&key, now))?,",
        "                EffectState::Running if self.charging.is_some() => self.store.write(|w| w.retry(&key, now))?,",
    ),
    (
        "card: a second request goes out while the shell has one",
        "src/card.rs",
        "        if self.charging.is_some() {\n            return Ok(None);\n        }\n        let now = self.clock.now();\n        // A request due far",
        "        let now = self.clock.now();\n        // A request due far",
    ),
    (
        "card: an unknown outcome is asked about at once",
        "src/card.rs",
        "            let due = print::after(self.clock.now(), wait)?;",
        "            let due = print::after(self.clock.now(), wait.min(Duration::ZERO))?;",
    ),
    (
        "card: an approval beyond what was asked is applied whole",
        "src/card.rs",
        "                    if approved.compare(info.amount)?.is_gt() { info.amount } else { approved };\n                let tip = approved",
        "                    approved;\n                let tip = approved",
    ),
    (
        "card: a sale approved doesn't settle its check",
        "src/card.rs",
        "            settle(w, &closing, &paid, meta, profile, locale)?;\n            Ok(())",
        "            let _ = (&closing, &paid, profile, locale);\n            Ok(())",
    ),
    (
        "card: a sale approved fires nothing to the kitchen",
        "src/card.rs",
        "            if let Some(firing) = &firing {\n                crate::kitchen::write_firing(w, firing, location, device, meta)?;\n            }",
        "            let _ = &firing;",
    ),
    (
        "card: a payment settled by hand loses its tip",
        "src/card.rs",
        "                amount: info.amount,\n                tip,\n                reference: None,",
        "                amount: info.amount,\n                tip: tip.filter(|_| false),\n                reference: None,",
    ),
    (
        "card: settling by hand needs no manager",
        "src/card.rs",
        "        let granted = self.gate(Request::of(Action::PaymentResolve), Some(payment.cast()))?;",
        "        let granted = self.gate(Request::of(Action::ReceiptReprint), Some(payment.cast()))?;",
    ),
    (
        "card: a tip beyond what it tips on is taken",
        "src/card.rs",
        "                    || tip.compare(amount).is_ok_and(core::cmp::Ordering::is_gt)",
        "                    || tip.compare(amount).is_ok_and(|_| false)",
    ),
    (
        "card: the terminal asks for the tip where the register does",
        "src/card.rs",
        "                Some(tipping) if tipping.prompt == TipPrompt::Terminal => {",
        "                Some(tipping) => {",
    ),
    (
        "tips: a preset is a percentage of the total, tax included",
        "src/card.rs",
        "        let base = share(paying, shown.subtotal.money.minor(), shown.total.money.minor())",
        "        let base = share(paying, shown.total.money.minor(), shown.total.money.minor())",
    ),
    (
        "tips: a preset's amount rounds down",
        "src/card.rs",
        "    let rounded = product.checked_add(denominator / 2)?.checked_div(denominator)?;",
        "    let rounded = product.checked_div(denominator)?;",
    ),
    (
        "refund: a card refund takes more than is left of it",
        "src/returns.rs",
        "            Some((card, _)) => Some(card.left.money),",
        "            Some((_, _)) => None,",
    ),
    (
        "refund: what is left of a card counts its tip",
        "src/returns.rs",
        "                    let mut left = captured.amount;",
        "                    let mut left = captured.amount.checked_add(captured.tip.unwrap_or(Money::zero(captured.amount.currency())))?;",
    ),
    (
        "refund: a card refund declined prints its receipt",
        "src/card.rs",
        "            if let Some(doc) = receipt.filter(|_| completes) {",
        "            if let Some(doc) = receipt {\n                let _ = completes;",
    ),
    (
        "refund: a card refund is tried again as its failed attempt",
        "src/card.rs",
        "        let attempt = failures.saturating_add(1);",
        "        let attempt = failures;",
    ),
    (
        "refund: a card refund hands back cash",
        "src/returns.rs",
        "        let handed = if card.is_some() {\n            Money::zero(total.currency())\n        } else {",
        "        let handed = if card.is_some() && card.is_none() {\n            Money::zero(total.currency())\n        } else {",
    ),
    (
        "report: tips aren't by who took them",
        "src/report.rs",
        "            match tips.iter_mut().find(|(member, _, _)| *member == payment.member) {",
        "            match tips.iter_mut().find(|(member, _, _)| *member == payment.member.filter(|_| false)) {",
    ),
    # Review fixes (ADR-0035).
    (
        "card: an answer that can't be recorded keeps the request with the shell",
        "src/card.rs",
        "        self.charging = None;\n        let bad = ",
        "        let bad = ",
    ),
    (
        "card: a released request stays with the shell",
        "src/card.rs",
        "        let Some(key) = self.charging.take() else { return Ok(()) };",
        "        let Some(key) = self.charging.clone() else { return Ok(()) };",
    ),
    (
        "refund: a card with no reference passes for cash",
        "src/returns.rs",
        "                Tender::Card => return Ok(None),",
        "                Tender::Card => {}",
    ),
    (
        "report: a card refund paid back in part owes nothing",
        "src/report.rs",
        "            let short = info.amount.checked_sub(paid)?;",
        "            let short = if refund.completed.is_some() { Money::zero(currency) } else { info.amount.checked_sub(paid)? };",
    ),
    # Stock (ADR-0036).
    (
        "stock: wasting isn't gated",
        "src/stock.rs",
        "        let gated = matches!(movement, Movement::Adjusted | Movement::Wasted);",
        "        let gated = matches!(movement, Movement::Adjusted);",
    ),
    (
        "stock: a count isn't gated",
        "src/stock.rs",
        "        self.record_stock(variant, command, true)",
        "        self.record_stock(variant, command, false)",
    ),
    (
        "stock: an item the catalog doesn't have is stocked",
        "src/stock.rs",
        "        if self.profile.variant(variant).is_none() {",
        "        if self.profile.variant(variant).is_none() && false {",
    ),
    (
        "stock: below zero isn't listed first",
        "src/stock.rs",
        "            b.on_hand.is_negative().cmp(&a.on_hand.is_negative()).then_with(|| a.name.cmp(&b.name))",
        "            a.name.cmp(&b.name)",
    ),
    (
        "stock: the history keeps what moved before the count",
        "src/stock.rs",
        "            (Some(count), _) => at > count.as_of,",
        "            (Some(_), _) => true,",
    ),
    (
        "stock: the history is oldest first",
        "src/stock.rs",
        "        lines.sort_by(|a, b| b.at.cmp(&a.at));",
        "        lines.sort_by(|a, b| a.at.cmp(&b.at));",
    ),
    (
        "stock: a count is expected against what sold after it",
        "src/stock.rs",
        "    let flows: Vec<Flow> = flows.iter().filter(|flow| flow.at <= as_of).copied().collect();",
        "    let flows: Vec<Flow> = flows.to_vec();",
    ),
    (
        "stock: a count is expected against what was recorded after its time",
        "src/stock.rs",
        "            EntryKind::Moved(..) => entry.at <= as_of,",
        "            EntryKind::Moved(..) => true,",
    ),
    (
        "stock: the day's waste is every day's",
        "src/stock.rs",
        "                if day.business_date_of(entry.at)? != date {\n                    continue;\n                }",
        "                let _ = day.business_date_of(entry.at)?;",
    ),
    (
        "stock: a return not put back is put back",
        "src/returns.rs",
        "        let unstocked = if restock { Vec::new() } else { self.unstocked(&loaded, returned)? };",
        "        let unstocked: Vec<(Id<keel_domain::refs::Variant>, Quantity)> = Vec::new();",
    ),
    (
        "stock: a return not put back wastes an untracked item",
        "src/returns.rs",
        "            if self.store.stock_unit(variant)? == Some(each.quantity.unit()) {",
        "            if self.store.stock_unit(variant)?.is_none_or(|unit| unit == each.quantity.unit()) {",
    ),
    (
        "stock: a count is as of the device's clock",
        "src/stock.rs",
        "        let as_of = as_of.unwrap_or_else(|| self.recorded_now());",
        "        let as_of = as_of.unwrap_or_else(|| self.clock.now());",
    ),
    (
        "stock: two counts as of one time leave each other out",
        "src/stock.rs",
        "            EntryKind::Counted { .. } => count_precedes(entry, count),",
        "            EntryKind::Counted { as_of: other, .. } => other < as_of,",
    ),
]
