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
        'change after removal applied',
        'src/order/state.rs',
        '        let refused = if !line.is_live() {',
        '        let refused = if false {',
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
        'change after fire unflagged',
        'src/order/state.rs',
        """        if line.status == LineStatus::Fired {
            self.conflict(meta, ConflictKind::ChangedAfterFire(id));
        }
""",
        '',
    ),
    (
        'removal of a pending line ignored',
        'src/order/state.rs',
        '            LineStatus::Pending => false,',
        '            LineStatus::Pending => return,',
    ),
    (
        'removal after fire unflagged',
        'src/order/state.rs',
        """        if fired {
            self.conflict(meta, ConflictKind::RemovedAfterFire(id));
        }
""",
        '',
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
        'a later close replaces the first',
        'src/order/state.rs',
        """    fn apply_ended(&mut self, meta: &EventMeta, status: OrderStatus) {
        if self.has_ended() {
            return;
        }
""",
        """    fn apply_ended(&mut self, meta: &EventMeta, status: OrderStatus) {
""",
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
        'commands ignore the location',
        'src/order/commands.rs',
        """        let info = self.info().ok_or(CommandError::NotCreated)?;
        if info.location != location {""",
        """        let info = self.info().ok_or(CommandError::NotCreated)?;
        if false {""",
    ),
    (
        'closed orders take commands',
        'src/order/commands.rs',
        """        let info = self.check_location(location)?;
        if *self.status() != OrderStatus::Active {""",
        """        let info = self.check_location(location)?;
        if false {""",
    ),
    (
        'no schema self-check',
        'src/order/commands.rs',
        """        check_recordable(&event)?;
        Ok(event)""",
        '        Ok(event)',
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
        'changes of unknown lines dropped silently',
        'src/order/state.rs',
        """        let frozen = self.frozen(id);
        let Some(line) = self.lines.iter_mut().find(|line| line.id == id) else {
            return self.conflict(meta, ConflictKind::UnknownLine(id));
        };
        let refused = if !line.is_live() {""",
        """        let frozen = self.frozen(id);
        let Some(line) = self.lines.iter_mut().find(|line| line.id == id) else {
            return;
        };
        let refused = if !line.is_live() {""",
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
        'src/order/commands.rs',
        '                Ok(OrderEvent::Voided { reason })',
        """                if self.live_lines().next().is_some() {
                    return Err(CommandError::HasLiveLines);
                }
                Ok(OrderEvent::Voided { reason })""",
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
        'orders come without a main check',
        'src/order/state.rs',
        """        self.checks.push(Check { id: self.main_check(), number: NonZeroU32::MIN, closed: None });
""",
        '',
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
        'allocations to unknown checks applied',
        'src/order/state.rs',
        """            let unknown =
                shares.iter().map(|share| share.check).find(|&check| self.check(check).is_none());""",
        '            let unknown: Option<Id<Check>> = None;',
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
        'new lines on no check',
        'src/order/state.rs',
        '            allocation: vec![CheckShare { check, shares: NonZeroU16::MIN }],',
        '            allocation: Vec::new(),',
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
        'allocation commands name unknown checks',
        'src/order/commands.rs',
        """                let check =
                    self.check(share.check).ok_or(CommandError::UnknownCheck(share.check))?;""",
        '                let Some(check) = self.check(share.check) else { continue };',
    ),
    (
        'allocation commands take removed lines',
        'src/order/commands.rs',
        """            if !line.is_live() {
                return Err(CommandError::LineNotLive(id));
            }
            for share in &shares {""",
        '            for share in &shares {',
    ),
    (
        'allocation commands change nothing',
        'src/order/commands.rs',
        """            self.check_not_frozen(line)?;
            if line.allocation() == shares.as_slice() {
                return Err(CommandError::NoChange);
            }""",
        '            self.check_not_frozen(line)?;',
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
        "every live line counted as on the check",
        "src/order/basket.rs",
        "            .filter(move |line| line.allocation().iter().any(|share| share.check == check))",
        "            .filter(move |line| line.allocation().iter().any(|share| share.check == check) || line.is_live())",
    ),
    (
        'check baskets include removed lines',
        'src/order/basket.rs',
        """        self.live_lines()
            .filter(move |line| line.allocation().iter().any(|share| share.check == check))""",
        """        self.lines()
            .iter()
            .filter(move |line| line.allocation().iter().any(|share| share.check == check))""",
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
    # Closing checks (ADR-0015): the fold.
    (
        "closes of unknown checks unreported",
        "src/order/state.rs",
        """        let Some(check) = self.checks.iter_mut().find(|check| check.id == id) else {
            return self.conflict(meta, ConflictKind::UnknownCheck(id));
        };""",
        """        let Some(check) = self.checks.iter_mut().find(|check| check.id == id) else {
            return;
        };""",
    ),
    (
        "a second close replaces the first",
        "src/order/state.rs",
        """        if check.closed.is_some() {
            return self.conflict(meta, ConflictKind::DuplicateClose(id));
        }
""",
        "",
    ),
    (
        "a second close unreported",
        "src/order/state.rs",
        "            return self.conflict(meta, ConflictKind::DuplicateClose(id));",
        "            return;",
    ),
    (
        "closes in another currency applied",
        "src/order/state.rs",
        """        if closed.total.currency() != currency {
            return self.conflict(meta, ConflictKind::CheckCurrencyMismatch(id));
        }
""",
        "",
    ),
    (
        "charges for lines off the check unreported",
        "src/order/state.rs",
        "                Some(_) => ConflictKind::ChargedOffCheck(charge.line),",
        "                Some(_) => continue,",
    ),
    (
        "charges for unknown lines unreported",
        "src/order/state.rs",
        "                None => ConflictKind::UnknownLine(charge.line),",
        "                None => continue,",
    ),
    (
        "removed lines count as on the check",
        "src/order/state.rs",
        """        let on_check =
            |line: &Line| line.is_live() && line.allocation.iter().any(|share| share.check == id);""",
        "        let on_check = |line: &Line| line.allocation.iter().any(|share| share.check == id);",
    ),
    (
        "lines left off stay on the closed check",
        "src/order/state.rs",
        """            if let Some(target) = self.open_check_for(meta, &holding)
                && let Some(line) = self.lines.get_mut(index)
            {
                for share in line.allocation.iter_mut().filter(|share| share.check == id) {
                    share.check = target;
                }
                line.allocation.sort_by_key(|share| share.check.to_bytes());
            }""",
        "            let _ = &holding;",
    ),
    (
        "lines left off unreported",
        "src/order/state.rs",
        "            self.conflict(meta, ConflictKind::LeftOffCheck(line_id));",
        "            let _ = line_id;",
    ),
    (
        "moved parts out of order",
        "src/order/state.rs",
        """                line.allocation.sort_by_key(|share| share.check.to_bytes());
""",
        "",
    ),
    (
        "moved parts join a check already holding the line",
        "src/order/state.rs",
        "        let holds = |check: Id<Check>| holding.iter().any(|share| share.check == check);",
        "        let holds = |_: Id<Check>| false;",
    ),
    (
        "no check opened when every check is closed",
        "src/order/state.rs",
        """        let id = meta.event_id.cast();
        if self.check(id).is_some() {
            return None;
        }
        self.checks.push(Check { id, number: self.next_number(), closed: None });
        Some(id)""",
        """        let _ = meta;
        None""",
    ),
    (
        "checks numbered from zero",
        "src/order/state.rs",
        "        let opened = u32::try_from(self.checks.len()).ok().and_then(|n| n.checked_add(1));",
        "        let opened = u32::try_from(self.checks.len()).ok();",
    ),
    (
        "forged identifiers open a second check",
        "src/order/state.rs",
        """        let id = meta.event_id.cast();
        if self.check(id).is_some() {
            return None;
        }""",
        "        let id = meta.event_id.cast();",
    ),
    (
        "new lines always go to the main check",
        "src/order/state.rs",
        "        let check = self.open_check_for(meta, &[]).unwrap_or(self.main_check());",
        "        let check = self.main_check();",
    ),
    (
        "new lines go to the last open check",
        "src/order/state.rs",
        "        if let Some(open) = self.checks.iter().find(|check| check.is_open() && !holds(check.id)) {",
        "        if let Some(open) = self.checks.iter().rev().find(|check| check.is_open() && !holds(check.id)) {",
    ),
    (
        "closed checks take new lines",
        "src/order/state.rs",
        "        if let Some(open) = self.checks.iter().find(|check| check.is_open() && !holds(check.id)) {",
        "        if let Some(open) = self.checks.iter().find(|check| !holds(check.id)) {",
    ),
    # Closed checks freeze their lines: the fold.
    (
        "changes to frozen lines unreported",
        "src/order/state.rs",
        """        if frozen && (changed.quantity.is_some() || changed.modifiers.is_some()) {
            self.conflict(meta, ConflictKind::ChangedOnClosedCheck(id));
        }
""",
        "",
    ),
    (
        "notes on frozen lines reported",
        "src/order/state.rs",
        "        if frozen && (changed.quantity.is_some() || changed.modifiers.is_some()) {",
        "        if frozen {",
    ),
    (
        "removals of frozen lines unreported",
        "src/order/state.rs",
        """        if fired {
            self.conflict(meta, ConflictKind::RemovedAfterFire(id));
        }
        if frozen {
            self.conflict(meta, ConflictKind::ChangedOnClosedCheck(id));
        }""",
        """        if fired {
            self.conflict(meta, ConflictKind::RemovedAfterFire(id));
        }""",
    ),
    (
        "voids of frozen lines unreported",
        "src/order/state.rs",
        """            line.status = LineStatus::Voided(reason.clone());
            if frozen {
                self.conflict(meta, ConflictKind::ChangedOnClosedCheck(id));
            }""",
        "            line.status = LineStatus::Voided(reason.clone());",
    ),
    (
        "comps of frozen lines unreported",
        "src/order/state.rs",
        """            line.comp = Some(reason.clone());
            if frozen {
                self.conflict(meta, ConflictKind::ChangedOnClosedCheck(id));
            }""",
        "            line.comp = Some(reason.clone());",
    ),
    (
        "repeated comps of frozen lines reported",
        "src/order/state.rs",
        """        if line.comp.is_none() {
            line.comp = Some(reason.clone());
            if frozen {
                self.conflict(meta, ConflictKind::ChangedOnClosedCheck(id));
            }
        }""",
        """        if line.comp.is_none() {
            line.comp = Some(reason.clone());
        }
        if frozen {
            self.conflict(meta, ConflictKind::ChangedOnClosedCheck(id));
        }""",
    ),
    (
        "frozen lines re-allocated",
        "src/order/state.rs",
        "                None if frozen || onto_closed => {",
        "                None if onto_closed => {",
    ),
    (
        "allocations onto closed checks applied",
        "src/order/state.rs",
        "                None if frozen || onto_closed => {",
        "                None if frozen => {",
    ),
    (
        "allocations on closed checks unreported",
        "src/order/state.rs",
        """                None if frozen || onto_closed => {
                    self.conflict(meta, ConflictKind::AllocatedOnClosedCheck(id));
                }""",
        "                None if frozen || onto_closed => {}",
    ),
    (
        "every line on a known check is frozen",
        "src/order/state.rs",
        """                .any(|share| self.check(share.check).is_some_and(|check| !check.is_open()))
    }""",
        """                .any(|share| self.check(share.check).is_some())
    }""",
    ),
    # Closing and reopening the order: the fold.
    (
        "orders closed with open checks unreported",
        "src/order/state.rs",
        """        for check in unpaid {
            self.conflict(meta, ConflictKind::ClosedWithOpenCheck(check));
        }""",
        "        let _ = unpaid;",
    ),
    (
        "empty open checks reported at a close",
        "src/order/state.rs",
        "            .filter(|check| check.is_open() && self.holds_live_line(check.id))",
        "            .filter(|check| check.is_open())",
    ),
    (
        "a closed order's second close reports again",
        "src/order/state.rs",
        """        if self.status != OrderStatus::Active {
            return;
        }
        self.status = OrderStatus::Closed;""",
        """        if self.has_ended() {
            return;
        }
        self.status = OrderStatus::Closed;""",
    ),
    (
        "reopening leaves checks closed",
        "src/order/state.rs",
        """        for check in &mut self.checks {
            check.closed = None;
        }""",
        "",
    ),
    (
        "reopening leaves the order closed",
        "src/order/state.rs",
        """        self.status = OrderStatus::Active;
        for check in &mut self.checks {""",
        "        for check in &mut self.checks {",
    ),
    (
        "voided orders reopen",
        "src/order/state.rs",
        """    fn apply_reopened(&mut self) {
        if self.has_ended() {
            return;
        }""",
        "    fn apply_reopened(&mut self) {",
    ),
    (
        "closed orders can't be voided",
        "src/order/state.rs",
        """    fn apply_ended(&mut self, meta: &EventMeta, status: OrderStatus) {
        if self.has_ended() {""",
        """    fn apply_ended(&mut self, meta: &EventMeta, status: OrderStatus) {
        if self.is_closed() {""",
    ),
    (
        "lines added to closed orders unreported",
        "src/order/state.rs",
        """        if self.is_closed() {
            self.conflict(meta, ConflictKind::AddedToClosedOrder(added.line));""",
        """        if self.has_ended() {
            self.conflict(meta, ConflictKind::AddedToClosedOrder(added.line));""",
    ),
    (
        "lines fired on closed orders unreported",
        "src/order/state.rs",
        "        let closed = self.is_closed();",
        "        let closed = self.has_ended();",
    ),
    # Closing and freezing: the commands.
    (
        "what frozen lines cost changes",
        "src/order/commands.rs",
        """                if changed.quantity.is_some() || changed.modifiers.is_some() {
                    self.check_not_frozen(line)?;
                }
""",
        "",
    ),
    (
        "notes on frozen lines refused",
        "src/order/commands.rs",
        """                if changed.quantity.is_some() || changed.modifiers.is_some() {
                    self.check_not_frozen(line)?;""",
        """                if true {
                    self.check_not_frozen(line)?;""",
    ),
    (
        "frozen lines removed",
        "src/order/commands.rs",
        "                self.check_not_frozen(self.pending_line(line)?)?;",
        "                self.pending_line(line)?;",
    ),
    (
        "frozen lines voided",
        "src/order/commands.rs",
        """                self.check_not_frozen(found)?;
                Ok(OrderEvent::LineVoided { line, reason })""",
        "                Ok(OrderEvent::LineVoided { line, reason })",
    ),
    (
        "frozen lines comped",
        "src/order/commands.rs",
        """                self.check_not_frozen(found)?;
                Ok(OrderEvent::LineComped { line, reason })""",
        "                Ok(OrderEvent::LineComped { line, reason })",
    ),
    (
        "frozen lines allocated",
        "src/order/commands.rs",
        """            self.check_not_frozen(line)?;
            if line.allocation() == shares.as_slice() {""",
        "            if line.allocation() == shares.as_slice() {",
    ),
    (
        "allocations onto closed checks allowed",
        "src/order/commands.rs",
        """                if !check.is_open() {
                    return Err(CommandError::CheckClosed(share.check));
                }""",
        "                let _ = check;",
    ),
    (
        "paid orders voided",
        "src/order/commands.rs",
        """            OrderCommand::Void(reason) => {
                if let Some(check) = self.closed_check() {
                    return Err(CommandError::CheckClosed(check));
                }""",
        "            OrderCommand::Void(reason) => {",
    ),
    (
        "paid orders abandoned",
        "src/order/commands.rs",
        """                if let Some(check) = self.closed_check() {
                    return Err(CommandError::CheckClosed(check));
                }
                Ok(OrderEvent::Abandoned)""",
        "                Ok(OrderEvent::Abandoned)",
    ),
    (
        "orders close with unpaid checks",
        "src/order/commands.rs",
        """                if let Some(check) = open {
                    return Err(CommandError::CheckOpen(check.id()));
                }""",
        "                let _ = open;",
    ),
    (
        "orders with nothing live close",
        "src/order/commands.rs",
        """            OrderCommand::Close => {
                if self.live_lines().next().is_none() {
                    return Err(CommandError::NothingToClose);
                }""",
        "            OrderCommand::Close => {",
    ),
    (
        "orders close only once every check is closed",
        "src/order/commands.rs",
        "                    .find(|check| check.is_open() && self.holds_live_line(check.id()));",
        "                    .find(|check| check.is_open());",
    ),
    (
        "orders reopen with nothing closed",
        "src/order/commands.rs",
        "                self.closed_check().ok_or(CommandError::NothingToReopen)?;",
        "                let _ = self.closed_check();",
    ),
    (
        "voided orders reopen by command",
        "src/order/commands.rs",
        "            OrderCommand::Reopen(reason) if *self.status() == OrderStatus::Closed => {",
        "            OrderCommand::Reopen(reason) if *self.status() != OrderStatus::Active => {",
    ),
    (
        "closed orders reopen from anywhere",
        "src/order/commands.rs",
        """                self.check_location(location)?;
                OrderEvent::Reopened { reason }""",
        "                OrderEvent::Reopened { reason }",
    ),
    (
        "closed checks close again",
        "src/order/commands.rs",
        """        if !found.is_open() {
            return Err(CommandError::CheckClosed(check));
        }
        if !self.holds_live_line(check) {""",
        "        if !self.holds_live_line(check) {",
    ),
    (
        "empty checks close",
        "src/order/commands.rs",
        """        if !self.holds_live_line(check) {
            return Err(CommandError::NothingToClose);
        }
        Ok(info)""",
        "        Ok(info)",
    ),
    # Closed checks' snapshots: the payload rules.
    (
        "snapshot lines in any order",
        "src/order/closing.rs",
        "            && ascending(self.lines.iter().map(|charge| charge.line.to_bytes()))",
        "            && true",
    ),
    (
        "snapshots with no lines",
        "src/order/closing.rs",
        "        let lines_valid = !self.lines.is_empty()",
        "        let lines_valid = true",
    ),
    (
        "nets above the gross",
        "src/order/closing.rs",
        "                    && at_most(charge.net, charge.gross)",
        "                    && true",
    ),
    (
        "negative nets",
        "src/order/closing.rs",
        "                    && !charge.net.is_negative()",
        "                    && true",
    ),
    (
        "negative line tax",
        "src/order/closing.rs",
        "                    && !charge.tax.is_negative()",
        "                    && true",
    ),
    # Not listed: a line's amounts in another currency than the total's. Comparing the net
    # with the gross, and adding up the nets and the taxes, refuse every such amount too, so
    # removing that check changes nothing decoding accepts.
    (
        "taxes in any order",
        "src/order/closing.rs",
        "        let taxes_valid = ascending(self.taxes.iter().map(|charge| charge.tax.to_bytes()))",
        "        let taxes_valid = true",
    ),
    (
        "taxes on nothing",
        "src/order/closing.rs",
        "                    && charge.taxable.is_positive()",
        "                    && true",
    ),
    (
        "negative taxes",
        "src/order/closing.rs",
        "                    && !charge.amount.is_negative()",
        "                    && true",
    ),
    (
        "taxes that don't add up",
        "src/order/closing.rs",
        "        if !taxes_valid || tax.is_none() || tax != taxes {",
        "        if !taxes_valid || tax.is_none() {",
    ),
    (
        "totals that don't add up",
        "src/order/closing.rs",
        "        if total != Some(self.total) {",
        "        if total.is_none() {",
    ),
    (
        "unpaid snapshots",
        "src/order/closing.rs",
        "        if self.total.is_positive() && self.payments.is_none() {",
        "        if false {",
    ),
    # Payments: the payload rules.
    (
        "payments of nothing initiated",
        "src/payment/events.rs",
        "            if !initiated.amount.is_positive() {",
        "            if false {",
    ),
    (
        "authorizations of nothing",
        "src/payment/events.rs",
        "            if !authorized.amount.is_positive() {",
        "            if false {",
    ),
    (
        "captures of nothing",
        "src/payment/events.rs",
        """        if !self.amount.is_positive() {
            return Err(PayloadError::Invalid("amount"));""",
        """        if false {
            return Err(PayloadError::Invalid("amount"));""",
    ),
    (
        "tips of nothing",
        "src/payment/events.rs",
        "        if self.tip.is_some_and(|tip| tip.currency() != currency || !tip.is_positive()) {",
        "        if self.tip.is_some_and(|tip| tip.currency() != currency) {",
    ),
    (
        "tips in another currency",
        "src/payment/events.rs",
        "        if self.tip.is_some_and(|tip| tip.currency() != currency || !tip.is_positive()) {",
        "        if self.tip.is_some_and(|tip| !tip.is_positive()) {",
    ),
    (
        "roundings of nothing",
        "src/payment/events.rs",
        "            .is_some_and(|rounding| rounding.currency() != currency || rounding.is_zero())",
        "            .is_some_and(|rounding| rounding.currency() != currency)",
    ),
    # Not listed: a cash rounding in another currency. What the customer paid then can't be
    # added up, so the capture is refused either way.
    (
        "roundings without cash",
        "src/payment/events.rs",
        "        (None, Some(_)) => return Err(PayloadError::Invalid(\"rounding\")),",
        "        (None, Some(_)) => None,",
    ),
    (
        "cash that doesn't cover what is paid",
        "src/payment/events.rs",
        "            cash.tendered.currency() == currency && !received.is_negative() && !change.is_negative()",
        "            cash.tendered.currency() == currency && !received.is_negative()",
    ),
    (
        "negative cash payments",
        "src/payment/events.rs",
        "            cash.tendered.currency() == currency && !received.is_negative() && !change.is_negative()",
        "            cash.tendered.currency() == currency && !change.is_negative()",
    ),
    # Payments: the fold.
    (
        "a second initiation replaces the first",
        "src/payment/state.rs",
        """            if self.info.is_some() {
                return self.conflict(meta, ConflictKind::DuplicateInitiation);
            }
""",
        "",
    ),
    (
        "outcomes from another location applied",
        "src/payment/state.rs",
        """        if meta.location != info.location {
            return self.conflict(meta, ConflictKind::WrongLocation);
        }
""",
        "",
    ),
    (
        "outcomes in another currency applied",
        "src/payment/state.rs",
        """        if amount.is_some_and(|amount| amount.currency() != currency) {
            return self.conflict(meta, ConflictKind::CurrencyMismatch);
        }
""",
        "",
    ),
    (
        "a second capture replaces the first",
        "src/payment/state.rs",
        """                self.conflict(meta, ConflictKind::OutOfTurn { after: Outcome::Captured, outcome });
                false""",
        """                self.conflict(meta, ConflictKind::OutOfTurn { after: Outcome::Captured, outcome });
                true""",
    ),
    (
        "outcomes after a capture unreported",
        "src/payment/state.rs",
        """                self.conflict(meta, ConflictKind::OutOfTurn { after: Outcome::Captured, outcome });
                false""",
        "                false",
    ),
    (
        "captures after a failure lost",
        "src/payment/state.rs",
        """                self.conflict(meta, ConflictKind::OutOfTurn { after, outcome });
                true""",
        """                self.conflict(meta, ConflictKind::OutOfTurn { after, outcome });
                false""",
    ),
    (
        "outcomes after a failure unreported",
        "src/payment/state.rs",
        """                self.conflict(meta, ConflictKind::OutOfTurn { after, outcome });
                true""",
        """                let _ = after;
                true""",
    ),
    (
        "out-of-turn outcomes named backwards",
        "src/payment/state.rs",
        "                self.conflict(meta, ConflictKind::OutOfTurn { after, outcome });",
        "                self.conflict(meta, ConflictKind::OutOfTurn { after: outcome, outcome: after });",
    ),
    (
        "a second authorization replaces the first",
        "src/payment/state.rs",
        "            | (Some(Outcome::Authorized), Outcome::Captured | Outcome::Failed | Outcome::Voided) => {",
        "            | (Some(Outcome::Authorized), _) => {",
    ),
    (
        "a void after a failure replaces it",
        "src/payment/state.rs",
        """            (Some(Outcome::Authorized), Outcome::Authorized)
            | (Some(Outcome::Failed | Outcome::Voided), Outcome::Failed | Outcome::Voided) => false,""",
        """            (Some(Outcome::Authorized), Outcome::Authorized) => false,
            (Some(Outcome::Failed | Outcome::Voided), Outcome::Failed | Outcome::Voided) => true,""",
    ),
    # Payments: the commands.
    (
        "cash authorized",
        "src/payment/commands.rs",
        """                if cash {
                    return Err(PaymentError::WrongTender);
                }
                if matches!(self.status(), PaymentStatus::Authorized(_)) {""",
        "                if matches!(self.status(), PaymentStatus::Authorized(_)) {",
    ),
    (
        "cards authorized twice",
        "src/payment/commands.rs",
        """                if matches!(self.status(), PaymentStatus::Authorized(_)) {
                    return Err(PaymentError::AlreadyAuthorized);
                }
""",
        "",
    ),
    (
        "authorizations for more than asked",
        "src/payment/commands.rs",
        "                if !at_most(authorized.amount, info.amount) {",
        "                if false {",
    ),
    (
        "authorizations in another currency",
        "src/payment/commands.rs",
        "                if !in_currency(authorized.amount) {",
        "                if false {",
    ),
    (
        "captures for more than authorized",
        "src/payment/commands.rs",
        "                    PaymentStatus::Authorized(authorized) => authorized.amount,",
        "                    PaymentStatus::Authorized(_) => info.amount,",
    ),
    (
        "captures for more than asked",
        "src/payment/commands.rs",
        "                if !at_most(captured.amount, limit) {",
        "                if false {",
    ),
    (
        "captures in another currency",
        "src/payment/commands.rs",
        "                if !in_currency(captured.amount) {",
        "                if false {",
    ),
    (
        "cash captured without the cash",
        "src/payment/commands.rs",
        "                if cash != captured.cash.is_some() || cash && captured.reference.is_some() {",
        "                if cash && captured.reference.is_some() {",
    ),
    (
        "cash captured with a processor reference",
        "src/payment/commands.rs",
        "                if cash != captured.cash.is_some() || cash && captured.reference.is_some() {",
        "                if cash != captured.cash.is_some() {",
    ),
    (
        "cash ended with a processor reference",
        "src/payment/commands.rs",
        "                if cash && ended.reference.is_some() =>",
        "                if false =>",
    ),
    (
        "resolved payments take outcomes",
        "src/payment/commands.rs",
        "        if !self.status().is_unresolved() {",
        "        if false {",
    ),
    (
        "payments take outcomes from anywhere",
        "src/payment/commands.rs",
        """        if info.location != location {
            return Err(PaymentError::WrongLocation);""",
        """        if false {
            return Err(PaymentError::WrongLocation);""",
    ),
    (
        "no payment self-check",
        "src/payment/commands.rs",
        """        check_recordable(&event)?;
        Ok(event)""",
        "        Ok(event)",
    ),
    # Checkout.
    (
        "other orders' payments counted",
        "src/checkout/mod.rs",
        "            .filter(|payment| payment.info().is_some_and(|info| info.order == order.id()))",
        "            .filter(|payment| payment.info().is_some())",
    ),
    (
        "payments counted twice",
        "src/checkout/mod.rs",
        """        payments.dedup_by_key(|payment| payment.id());
""",
        "",
    ),
    (
        "payments in other currencies counted",
        "src/checkout/mod.rs",
        "            .filter(|payment| payment.info().is_some_and(|info| info.amount.currency() == currency))",
        "            .filter(|payment| payment.info().is_some())",
    ),
    (
        "authorized payments resolved",
        "src/checkout/mod.rs",
        "            .filter(|payment| payment.is_unresolved())",
        "            .filter(|payment| payment.is_unresolved() && payment.status().outcome().is_none())",
    ),
    (
        "closed checks repriced",
        "src/checkout/mod.rs",
        "            Some(closed) => closed.total,",
        "            Some(_) => self.price(check)?.total,",
    ),
    (
        "tips count toward the check",
        "src/checkout/mod.rs",
        """        let captured = Money::sum(currency, counted.iter().map(|(_, captured)| captured.amount))?;
        let tips = Money::sum(currency, counted.iter().filter_map(|(_, captured)| captured.tip))?;""",
        """        let captured = Money::sum(currency, counted.iter().map(|(_, captured)| captured.received().unwrap_or(captured.amount)))?;
        let tips = Money::sum(currency, counted.iter().filter_map(|(_, captured)| captured.tip))?;""",
    ),
    (
        "payments start on closed checks",
        "src/checkout/mod.rs",
        """        if !found.is_open() {
            return Err(CommandError::CheckClosed(check).into());
        }""",
        "        let _ = found;",
    ),
    (
        "payments start beside unresolved ones",
        "src/checkout/mod.rs",
        """        if let Some(&unresolved) = balance.unresolved.first() {
            return Err(CheckoutError::Unresolved(unresolved));
        }
""",
        "",
    ),
    (
        "payments for more than the check owes",
        "src/checkout/mod.rs",
        "        if amount.compare(balance.due)? == Ordering::Greater {",
        "        if false {",
    ),
    (
        "payments in another currency start",
        "src/checkout/mod.rs",
        """        if amount.currency() != info.currency {
            return Err(CheckoutError::WrongCurrency);""",
        """        if false {
            return Err(CheckoutError::WrongCurrency);""",
    ),
    (
        "payment identifiers reused",
        "src/checkout/mod.rs",
        "        if self.payments.iter().any(|payment| payment.id() == id) {",
        "        if false {",
    ),
    (
        "payments start on any order",
        "src/checkout/mod.rs",
        """        let info = self.order.check_active(location)?;
        let found = self.order.check(check).ok_or(CommandError::UnknownCheck(check))?;""",
        """        let _ = location;
        let info = self.order.info().ok_or(CommandError::NotCreated)?;
        let found = self.order.check(check).ok_or(CommandError::UnknownCheck(check))?;""",
    ),
    (
        "checks close beside unresolved payments",
        "src/checkout/mod.rs",
        """        if let Some(unresolved) = self.payments_on(check).find(|payment| payment.is_unresolved()) {
            return Err(CheckoutError::Unresolved(unresolved.id()));
        }
""",
        "",
    ),
    (
        "checks close unpaid",
        "src/checkout/mod.rs",
        """        if due.is_positive() {
            return Err(CheckoutError::NotCovered { due });""",
        """        if false {
            return Err(CheckoutError::NotCovered { due });""",
    ),
    (
        "checks close a cent short",
        "src/checkout/mod.rs",
        """        if due.is_positive() {
            return Err(CheckoutError::NotCovered { due });""",
        """        if due.minor() > 1 {
            return Err(CheckoutError::NotCovered { due });""",
    ),
    (
        "snapshot lines unsorted",
        "src/checkout/mod.rs",
        """        lines.sort_by_key(|charge| charge.line.to_bytes());
""",
        "",
    ),
    (
        "snapshots list taxes on nothing",
        "src/checkout/mod.rs",
        "            .filter(|tax| tax.taxable.is_positive())",
        "            .filter(|_| true)",
    ),
    (
        "snapshots list every payment on the check",
        "src/checkout/mod.rs",
        "        let payments = IdSet::new(counted.into_iter().map(|(id, _)| id)).ok();",
        """        let _ = counted;
        let payments = IdSet::new(self.payments_on(check).map(Payment::id)).ok();""",
    ),
    (
        "snapshots total what was captured",
        "src/checkout/mod.rs",
        "            total: totals.total,",
        "            total: captured,",
    ),
    (
        "snapshots record another version",
        "src/checkout/mod.rs",
        "            rules_version: self.rules_version,",
        "            rules_version: RulesVersion::from_bytes([0; 32]),",
    ),
    (
        "payments for unknown checks unreported",
        "src/checkout/mod.rs",
        "                issues.push(Issue::UnknownCheck(id));",
        "                let _ = id;",
    ),
    (
        "payments in other currencies unreported",
        "src/checkout/mod.rs",
        "                issues.push(Issue::WrongCurrency(id));",
        "                let _ = id;",
    ),
    (
        "unresolved payments on voided orders unreported",
        "src/checkout/mod.rs",
        "            let live = payment.captured().is_some() || payment.is_unresolved();",
        "            let live = payment.captured().is_some();",
    ),
    (
        "ended payments reported",
        "src/checkout/mod.rs",
        "            let live = payment.captured().is_some() || payment.is_unresolved();",
        "            let live = true;",
    ),
    (
        "payments after a close unreported",
        "src/checkout/mod.rs",
        "                issues.push(Issue::AfterClose(id));",
        "                let _ = id;",
    ),
    (
        "settling payments reported after the close",
        "src/checkout/mod.rs",
        "            if live && check.and_then(Check::closed).is_some_and(|closed| !settled(closed)) {",
        """            if live && check.and_then(Check::closed).is_some() {
                let _ = settled;""",
    ),
    (
        "overpayments unreported",
        "src/checkout/mod.rs",
        "                issues.push(Issue::Overpaid { check: check.id(), by: due.checked_neg()? });",
        "                let _ = due;",
    ),
    (
        "open checks reported underpaid",
        "src/checkout/mod.rs",
        "            } else if due.is_positive() && !check.is_open() {",
        "            } else if due.is_positive() {",
    ),
    (
        "underpayments unreported",
        "src/checkout/mod.rs",
        "                issues.push(Issue::Underpaid { check: check.id(), by: due });",
        "                let _ = due;",
    ),
    (
        "voided orders ignored",
        "src/checkout/mod.rs",
        "        let ended = matches!(self.order.status(), OrderStatus::Voided(_) | OrderStatus::Abandoned);",
        "        let ended = false;",
    ),
]
