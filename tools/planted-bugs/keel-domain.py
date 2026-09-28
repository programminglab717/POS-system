"""Planted bugs for keel-domain: see run.py."""

BUGS = [
    # Codecs.
    (
        "unknown fields accepted",
        "src/codec.rs",
        "            Some((key, _, _)) => Err(PayloadError::UnknownField(key.to_string())),",
        "            Some(_) => Ok(()),",
    ),
    (
        "null never clears",
        "src/codec.rs",
        "                Value::Null => Ok(Change::Clear),",
        "                Value::Null => Err(PayloadError::Invalid(name)),",
    ),
    (
        "text one character too long",
        "src/codec.rs",
        """        && text.chars().count() <= max
""",
        """        && text.chars().count() <= max.saturating_add(1)
""",
    ),
    (
        "control characters allowed in notes",
        "src/codec.rs",
        "(line_feeds && c == '\\n')",
        "line_feeds",
    ),
    (
        "reason codes may be 33 bytes",
        "src/codec.rs",
        """    text.len() <= 32
""",
        """    text.len() <= 33
""",
    ),
    (
        "id sets may repeat when built",
        "src/codec.rs",
        "        if ids.is_empty() || repeats {",
        "        if ids.is_empty() {",
    ),
    (
        "id sets may be unsorted in payloads",
        "src/codec.rs",
        "            [left, right] => left.to_bytes() < right.to_bytes(),",
        "            [left, right] => left.to_bytes() != right.to_bytes(),",
    ),
    # Payload rules.
    (
        "negative unit price accepted",
        "src/order/events.rs",
        "    if added.item.unit_price.is_negative() {",
        "    if false {",
    ),
    (
        "zero quantity accepted",
        "src/order/events.rs",
        "    if !added.quantity.is_positive() {",
        "    if added.quantity.is_negative() {",
    ),
    (
        "line modifiers in another currency accepted",
        "src/order/events.rs",
        "    if !added.modifiers.iter().all(|modifier| modifier.is_priced_in(currency)) {",
        "    if false {",
    ),
    (
        "empty attribute change accepted",
        "src/order/events.rs",
        """        owner: fields.change(6, "owner")?,
    };
    if changed.is_empty() {""",
        """        owner: fields.change(6, "owner")?,
    };
    if false {""",
    ),
    (
        "empty line change accepted",
        "src/order/events.rs",
        """        notes: fields.change(6, "notes")?,
    };
    if changed.is_empty() {""",
        """        notes: fields.change(6, "notes")?,
    };
    if false {""",
    ),
    (
        "changed quantity may be zero",
        "src/order/events.rs",
        "    if changed.quantity.is_some_and(|quantity| !quantity.is_positive()) {",
        "    if false {",
    ),
    (
        "modifier price may be negative",
        "src/order/types.rs",
        "        let prices_valid = !modifier.unit_price.is_negative()",
        "        let prices_valid = true",
    ),
    (
        "nested modifier currency unchecked",
        "src/order/types.rs",
        """            && self.modifiers.iter().all(|modifier| modifier.is_priced_in(currency))
""",
        """
""",
    ),
    # The fold.
    (
        "second creation replaces the first",
        "src/order/state.rs",
        "            return self.conflict(meta, ConflictKind::DuplicateCreation);",
        "            self.conflict(meta, ConflictKind::DuplicateCreation);",
    ),
    (
        "events before creation dropped silently",
        "src/order/state.rs",
        "            return self.conflict(meta, ConflictKind::BeforeCreation);",
        "            return;",
    ),
    (
        "events from another location applied",
        "src/order/state.rs",
        "        if meta.location != info.location {",
        "        if false {",
    ),
    (
        "duplicate line replaces the first",
        "src/order/state.rs",
        "            return self.conflict(meta, ConflictKind::DuplicateLine(added.line));",
        "            self.lines.retain(|line| line.id != added.line);",
    ),
    (
        "line in another currency added",
        "src/order/state.rs",
        "            return self.conflict(meta, ConflictKind::CurrencyMismatch(added.line));",
        "            self.conflict(meta, ConflictKind::CurrencyMismatch(added.line));",
    ),
    (
        "line added to a closed order unflagged",
        "src/order/state.rs",
        """            self.conflict(meta, ConflictKind::AddedToClosedOrder(added.line));
""",
        """
""",
    ),
    (
        "change after removal applied",
        "src/order/state.rs",
        "        let conflict = if !line.is_live() {",
        "        let conflict = if false {",
    ),
    (
        "change in another unit applied",
        "src/order/state.rs",
        "        } else if changed.quantity.is_some_and(|quantity| quantity.unit() != line.quantity.unit()) {",
        "        } else if false {",
    ),
    (
        "change in another currency applied",
        "src/order/state.rs",
        "            .is_some_and(|modifiers| modifiers != currency)",
        "            .is_some_and(|_| false)",
    ),
    (
        "change after fire unflagged",
        "src/order/state.rs",
        "            (line.status == LineStatus::Fired).then_some(ConflictKind::ChangedAfterFire(id))",
        "            None",
    ),
    (
        "removal of a pending line ignored",
        "src/order/state.rs",
        "            LineStatus::Pending => line.status = LineStatus::Removed,",
        "            LineStatus::Pending => {}",
    ),
    (
        "removal after fire unflagged",
        "src/order/state.rs",
        """                self.conflict(meta, ConflictKind::RemovedAfterFire(id));
""",
        """
""",
    ),
    (
        "fire revives a removed line",
        "src/order/state.rs",
        "                self.conflict(meta, ConflictKind::FiredAfterRemoval(id));",
        "                line.status = LineStatus::Fired;",
    ),
    (
        "fire on a closed order unflagged",
        "src/order/state.rs",
        """                    self.conflict(meta, ConflictKind::FiredOnClosedOrder(id));
""",
        """
""",
    ),
    (
        "void overrides a removal",
        "src/order/state.rs",
        """        if line.is_live() {
            line.status = LineStatus::Voided(reason.clone());""",
        """        if true {
            line.status = LineStatus::Voided(reason.clone());""",
    ),
    (
        "comp of a removed line applied",
        "src/order/state.rs",
        """        if !line.is_live() {
            return self.conflict(meta, ConflictKind::CompedAfterRemoval(id));""",
        """        if false {
            return self.conflict(meta, ConflictKind::CompedAfterRemoval(id));""",
    ),
    (
        "last comp wins",
        "src/order/state.rs",
        "        if line.comp.is_none() {",
        "        if true {",
    ),
    (
        "a later close replaces the first",
        "src/order/state.rs",
        """        if self.is_closed() {
            return;
        }
""",
        "",
    ),
    (
        "abandoning with lines unflagged",
        "src/order/state.rs",
        """            self.conflict(meta, ConflictKind::AbandonedWithLines);
""",
        """
""",
    ),
    (
        "mode changes ignored",
        "src/order/state.rs",
        "            info.mode = mode;",
        "            let _ = mode;",
    ),
    (
        "first attribute setting wins",
        "src/order/state.rs",
        "                *field = change.into_option();",
        """                if field.is_none() {
                    *field = change.into_option();
                }""",
    ),
    # Commands.
    (
        "commands ignore the location",
        "src/order/commands.rs",
        "                if info.location != location {",
        "                if false {",
    ),
    (
        "closed orders take commands",
        "src/order/commands.rs",
        "                if *self.status() != OrderStatus::Active {",
        "                if false {",
    ),
    (
        "no schema self-check",
        "src/order/commands.rs",
        "        OrderEvent::decode(&schema, &payload).map_err(|error| match error {",
        """        let _ = (&schema, &payload);
        Ok::<(), DecodeError>(()).map_err(|error| match error {""",
    ),
    (
        "existing line ids accepted",
        "src/order/commands.rs",
        "                    return Err(CommandError::LineExists(added.line));",
        "                    let _ = 0;",
    ),
    (
        "added line currency unchecked",
        "src/order/commands.rs",
        "                if !priced_in_currency {",
        "                if false {",
    ),
    (
        "no-op attribute changes accepted",
        "src/order/commands.rs",
        """        && changes(info.owner.as_ref(), changed.owner.as_ref());
    if empty || !all_change {""",
        """        && changes(info.owner.as_ref(), changed.owner.as_ref());
    if empty && !all_change {""",
    ),
    (
        "no-op line changes accepted",
        "src/order/commands.rs",
        """        && changes(line.notes(), changed.notes.as_ref());
    if empty || !all_change {""",
        """        && changes(line.notes(), changed.notes.as_ref());
    if empty && !all_change {""",
    ),
    (
        "changed quantity unit unchecked",
        "src/order/commands.rs",
        "        return Err(CommandError::WrongUnit(line.id()));",
        "        let _ = line.id();",
    ),
    (
        "changed modifier currency unchecked",
        "src/order/commands.rs",
        "    if !modifiers_in_currency {",
        "    if false {",
    ),
    (
        "fired lines treated as pending",
        "src/order/commands.rs",
        "        if *line.status() != LineStatus::Pending {",
        "        if !line.is_live() {",
    ),
    (
        "pending lines can be voided",
        "src/order/commands.rs",
        "                if *found.status() != LineStatus::Fired {",
        "                if !found.is_live() {",
    ),
    (
        "removed lines can be comped",
        "src/order/commands.rs",
        """                if !found.is_live() {
                    return Err(CommandError::LineNotLive(line));""",
        """                if false {
                    return Err(CommandError::LineNotLive(line));""",
    ),
    (
        "a line can be comped twice",
        "src/order/commands.rs",
        "                if found.comp().is_some() {",
        "                if false {",
    ),
    (
        "orders with lines can be abandoned",
        "src/order/commands.rs",
        "                if self.live_lines().next().is_some() {",
        "                if false {",
    ),
    # The fold's entry point.
    (
        "events of other streams folded",
        "src/aggregate.rs",
        "        body.stream.kind.as_str() == A::Event::STREAM && body.stream.id == aggregate.stream_id();",
        "        true;",
    ),
    # Aimed at the fold model, faulty commands and the canonical order.
    (
        "void of a pending line ignored",
        "src/order/state.rs",
        """        if line.is_live() {
            line.status = LineStatus::Voided(reason.clone());""",
        """        if line.status == LineStatus::Fired {
            line.status = LineStatus::Voided(reason.clone());""",
    ),
    (
        "attribute changes on closed orders ignored",
        "src/order/state.rs",
        "                    Order::apply_attributes(info, changed);",
        """                    if self.status == OrderStatus::Active {
                        Order::apply_attributes(info, changed);
                    }""",
    ),
    (
        "changes of unknown lines dropped silently",
        "src/order/state.rs",
        """        let id = changed.line;
        let Some(line) = self.lines.iter_mut().find(|line| line.id == id) else {
            return self.conflict(meta, ConflictKind::UnknownLine(id));""",
        """        let id = changed.line;
        let Some(line) = self.lines.iter_mut().find(|line| line.id == id) else {
            return;""",
    ),
    (
        "stage open only when every live line is pending",
        "src/order/state.rs",
        "        } else if live.any(|line| line.status == LineStatus::Pending) {",
        "        } else if live.all(|line| line.status == LineStatus::Pending) {",
    ),
    (
        "fired lines can't be comped in the fold",
        "src/order/state.rs",
        """        if !line.is_live() {
            return self.conflict(meta, ConflictKind::CompedAfterRemoval(id));""",
        """        if line.status != LineStatus::Pending {
            return self.conflict(meta, ConflictKind::CompedAfterRemoval(id));""",
    ),
    (
        "only a fire's first line is fired",
        "src/order/state.rs",
        """                for line in lines.iter() {
                    self.apply_line_fired(meta, line);""",
        """                for line in lines.iter().take(1) {
                    self.apply_line_fired(meta, line);""",
    ),
    (
        "abandoning with only removed lines flagged",
        "src/order/state.rs",
        "self.lines.iter().any(|line| line.is_live() || line.fired);",
        "!self.lines.is_empty();",
    ),
    (
        "fire commands check only the first line",
        "src/order/commands.rs",
        """                for line in lines.iter() {
                    self.pending_line(line)?;""",
        """                for line in lines.iter().take(1) {
                    self.pending_line(line)?;""",
    ),
    (
        "orders with live lines can't be voided",
        "src/order/commands.rs",
        "            OrderCommand::Void(reason) => Ok(OrderEvent::Voided { reason }),",
        """            OrderCommand::Void(reason) => {
                if self.live_lines().next().is_some() {
                    return Err(CommandError::HasLiveLines);
                }
                Ok(OrderEvent::Voided { reason })
            }""",
    ),
    (
        "orders with removed lines can't be abandoned",
        "src/order/commands.rs",
        """                if self.live_lines().next().is_some() {
                    return Err(CommandError::HasLiveLines);""",
        """                if !self.lines().is_empty() {
                    return Err(CommandError::HasLiveLines);""",
    ),
    (
        "undecodable events reported as unknown schemas",
        "src/aggregate.rs",
        "        Err(reason) => aggregate.skip(&meta, &body.schema, &reason),",
        "        Err(_) => aggregate.skip(&meta, &body.schema, &DecodeError::UnknownSchema),",
    ),
    (
        "provisional order puts position before device",
        "src/aggregate.rs",
        "    (a.hlc, a.origin_device, a.origin_seq).cmp(&(b.hlc, b.origin_device, b.origin_seq))",
        "    (a.hlc, a.origin_seq, a.origin_device).cmp(&(b.hlc, b.origin_seq, b.origin_device))",
    ),
    (
        "provisional order ignores the device",
        "src/aggregate.rs",
        "    (a.hlc, a.origin_device, a.origin_seq).cmp(&(b.hlc, b.origin_device, b.origin_seq))",
        "    (a.hlc, a.origin_seq).cmp(&(b.hlc, b.origin_seq))",
    ),
    # Abandonment needs that nothing was fired.
    (
        "abandoning ignores fired lines",
        "src/order/commands.rs",
        ".find(|line| line.was_fired())",
        ".find(|_| false)",
    ),
    (
        "fires after removal forgotten",
        "src/order/state.rs",
        """        line.fired = true;
        match line.status {
            LineStatus::Pending => {
                line.status = LineStatus::Fired;""",
        """        match line.status {
            LineStatus::Pending => {
                line.fired = true;
                line.status = LineStatus::Fired;""",
    ),
    (
        "abandonment flag ignores fired lines",
        "src/order/state.rs",
        "self.lines.iter().any(|line| line.is_live() || line.fired);",
        "self.lines.iter().any(|line| line.is_live());",
    ),
    # Pricing an order.
    (
        "baskets include removed lines",
        "src/order/basket.rs",
        "let lines = self.live_lines().map(|line| priced(line, None)).collect();",
        "let lines = self.lines().iter().map(|line| priced(line, None)).collect();",
    ),
    (
        "baskets forget comps",
        "src/order/basket.rs",
        "comped: line.comp().is_some(),",
        "comped: false,",
    ),
    (
        "baskets drop nested modifiers",
        "src/order/basket.rs",
        "modifiers: chosen.modifiers.iter().map(modifier).collect(),",
        "modifiers: Vec::new(),",
    ),
    (
        "baskets ignore modifier quantities",
        "src/order/basket.rs",
        "quantity: NonZeroU32::from(chosen.quantity),",
        "quantity: NonZeroU32::MIN,",
    ),
    (
        "baskets eat everything on the premises",
        "src/order/basket.rs",
        "let dining = if info.mode == Mode::DineIn { Dining::OnPremises } else { Dining::ToGo };",
        "let dining = if info.mode == Mode::DineIn { Dining::OnPremises } else { Dining::OnPremises };",
    ),
    # Checks and splits (ADR-0015).
    (
        "orders come without a main check",
        "src/order/state.rs",
        """        self.checks.push(Check { id: self.main_check(), number: NonZeroU32::MIN });
""",
        "",
    ),
    (
        "check numbers count from zero",
        "src/order/state.rs",
        "let opened = u32::try_from(self.checks.len()).ok().and_then(|n| n.checked_add(1));",
        "let opened = u32::try_from(self.checks.len()).ok();",
    ),
    (
        "checks opened twice",
        "src/order/state.rs",
        """        if self.check(id).is_some() {
            return self.conflict(meta, ConflictKind::DuplicateCheck(id));
        }""",
        "",
    ),
    (
        "allocations to unknown checks applied",
        "src/order/state.rs",
        """            let unknown = shares
                .iter()
                .map(|share| share.check)
                .find(|&check| !self.checks.iter().any(|known| known.id == check));""",
        "            let unknown: Option<Id<Check>> = None;",
    ),
    (
        "unknown checks reported for every line",
        "src/order/state.rs",
        "if !unknown_checks.contains(&check) {",
        "if true {",
    ),
    (
        "allocations of removed lines applied",
        "src/order/state.rs",
        """            if !line.is_live() {
                continue;
            }
            match unknown {""",
        """            match unknown {""",
    ),
    (
        "allocations add to the old shares",
        "src/order/state.rs",
        "None => line.allocation = shares,",
        "None => line.allocation.extend(shares),",
    ),
    (
        "new lines on no check",
        "src/order/state.rs",
        "allocation: vec![CheckShare { check: self.main_check(), shares: NonZeroU16::MIN }],",
        "allocation: Vec::new(),",
    ),
    (
        "checks opened again by command",
        "src/order/commands.rs",
        """                if self.check(check).is_some() {
                    return Err(CommandError::CheckExists(check));
                }""",
        "",
    ),
    (
        "allocation commands name unknown checks",
        "src/order/commands.rs",
        "shares.iter().find(|share| self.check(share.check).is_none())",
        "shares.iter().find(|_| false)",
    ),
    (
        "allocation commands take removed lines",
        "src/order/commands.rs",
        """                    if !line.is_live() {
                        return Err(CommandError::LineNotLive(id));
                    }""",
        "",
    ),
    (
        "allocation commands change nothing",
        "src/order/commands.rs",
        """                    if line.allocation() == shares.as_slice() {
                        return Err(CommandError::NoChange);
                    }""",
        "",
    ),
    (
        "check baskets ignore shares",
        "src/order/basket.rs",
        "let share = (allocation.len() > 1).then(|| Share {",
        "let share = (allocation.len() > usize::MAX).then(|| Share {",
    ),
    (
        "check baskets weigh shares in reverse",
        "src/order/basket.rs",
        "weights: allocation.iter().map(|share| u64::from(share.shares.get())).collect(),",
        "weights: allocation.iter().rev().map(|share| u64::from(share.shares.get())).collect(),",
    ),
    (
        "check baskets hold every live line",
        "src/order/basket.rs",
        "let index = allocation.iter().position(|share| share.check == check)?;",
        "let index = allocation.iter().position(|share| share.check == check).unwrap_or(0);",
    ),
    (
        "check baskets include removed lines",
        "src/order/basket.rs",
        """        let lines = self
            .live_lines()
            .filter_map(|line| {""",
        """        let lines = self
            .lines()
            .iter()
            .filter_map(|line| {""",
    ),
    (
        "check baskets for checks the order doesn't have",
        "src/order/basket.rs",
        """        self.check(check)?;
""",
        "",
    ),
    (
        "allocations decoded out of order",
        "src/order/checks.rs",
        "!allocations.is_empty() && ascending && lowest_terms",
        "!allocations.is_empty() && lowest_terms",
    ),
    (
        "allocations decoded in higher terms",
        "src/order/checks.rs",
        "!allocations.is_empty() && ascending && lowest_terms",
        "!allocations.is_empty() && ascending",
    ),
    (
        "empty allocations decoded",
        "src/order/checks.rs",
        "!allocations.is_empty() && ascending && lowest_terms",
        "ascending && lowest_terms",
    ),
    (
        "allocations not reduced to lowest terms",
        "src/order/checks.rs",
        """            let divisor = group.iter().fold(0, |divisor, allocation| {
                greatest_common_divisor(divisor, allocation.shares.get())
            });""",
        "            let divisor = 1;",
    ),
    (
        "allocations built with a line on a check twice",
        "src/order/checks.rs",
        "if allocations.is_empty() || repeats {",
        "if allocations.is_empty() {",
    ),
]
