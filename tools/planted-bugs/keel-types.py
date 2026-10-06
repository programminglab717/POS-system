"""Planted bugs for keel-types: see run.py."""

BUGS = [
    # Money, rounding and allocation.
    (
        "original naive mul_decimal (double rounding)",
        "src/money.rs",
        "let minor = mode.round_product(self.minor, factor).ok_or(MoneyError::Overflow)?;",
        """let product = Decimal::from(self.minor).checked_mul(factor).ok_or(MoneyError::Overflow)?;
        let minor = i64::try_from(mode.round(product, 0)).map_err(|_| MoneyError::Overflow)?;""",
    ),
    (
        "half-even picks the odd neighbour",
        "src/rounding.rs",
        "(discarded == Discarded::Half && truncated_is_odd)",
        "(discarded == Discarded::Half && !truncated_is_odd)",
    ),
    (
        "allocation ties go to the later part",
        "src/money.rs",
        "b.0.cmp(&a.0).then(a.1.cmp(&b.1))",
        "b.0.cmp(&a.0).then(b.1.cmp(&a.1))",
    ),
    (
        "parse accepts a leading +",
        "src/fixed_point.rs",
        "let (negative, unsigned) = match text.strip_prefix('-') {",
        """let text = text.strip_prefix('+').unwrap_or(text);
        let (negative, unsigned) = match text.strip_prefix('-') {""",
    ),
    (
        "order-dependent sum",
        "src/money.rs",
        """let mut total: i128 = 0;
        for amount in amounts {""",
        """return amounts.into_iter().try_fold(Money::zero(currency), Money::checked_add);
        #[allow(unreachable_code, reason = "mutant")]
        let mut total: i128 = 0;
        for amount in amounts {""",
    ),
    # Quantities and rounding helpers.
    (
        "pound off by one in the 8th digit",
        "src/quantity.rs",
        'Pound: Mass, "lb", "pound", 45_359_237 / 10^5;',
        'Pound: Mass, "lb", "pound", 45_359_236 / 10^5;',
    ),
    (
        "conversion swaps the scales",
        "src/quantity.rs",
        "let micros = pow10(to_scale)",
        "let micros = pow10(from_scale)",
    ),
    (
        "half-way test compares with the whole divisor",
        "src/rounding.rs",
        "Some(match remainder.cmp(&divisor.checked_sub(remainder)?) {",
        "Some(match remainder.cmp(&divisor) {",
    ),
    (
        "round_to_places keeps one place too many",
        "src/quantity.rs",
        "let dropped = Quantity::DECIMAL_PLACES.saturating_sub(decimal_places);",
        "let dropped = Quantity::DECIMAL_PLACES.saturating_sub(decimal_places + 1);",
    ),
    (
        "exact conversion accepts a rounded result",
        "src/quantity.rs",
        "if down == up {",
        "if down == up || true {",
    ),
    # Clocks and identifiers.
    (
        "observe ignores the remote counter on a wall-time tie",
        "src/hlc.rs",
        "advance(wall, last_logical.max(remote.logical()))?",
        "advance(wall, last_logical)?",
    ),
    (
        "observe has no drift guard",
        "src/hlc.rs",
        "if remote_wall > physical.saturating_add(self.max_forward_drift_ms) {",
        "if false {",
    ),
    (
        "tick restarts the counter when physical time merely equals the last wall time",
        "src/hlc.rs",
        "let next = if physical > last_wall {",
        "let next = if physical >= last_wall {",
    ),
    (
        "id counter may overflow into the version bits",
        "src/id.rs",
        ".filter(|&counter| counter <= MAX_COUNTER)",
        ".filter(|&counter| counter <= 0xFFFF)",
    ),
    (
        "ids follow the clock backwards",
        "src/id.rs",
        "Some((last_ms, last_counter)) if now_ms <= last_ms => {",
        "Some((last_ms, last_counter)) if now_ms == last_ms => {",
    ),
    # Business dates.
    (
        "naive business date (local date, or the day before)",
        "src/business_date.rs",
        """        for _ in 0..8 {
            if instant < self.start_of(date)? {
                date = date.previous()?;
            } else if instant >= self.start_of(date.next()?)? {
                date = date.next()?;
            } else {
                return Ok(date);
            }
        }
        Err(BusinessDateError::OutOfRange)
    }""",
        """        // Naive: the local date, or the day before if the instant precedes that date's start.
        if instant >= self.start_of(date)? { Ok(date) } else { date.previous() }
    }""",
    ),
    # Locales (ADR-0023).
    (
        "locales: digits are grouped from the left",
        "src/locale.rs",
        "let left = integer.len().saturating_sub(index);",
        "let left = index;",
    ),
    (
        "locales: the minus sign follows the symbol",
        "src/locale.rs",
        """        if amount.minor() < 0 {
            text.push(self.0.minus);
        }
        text.push_str(symbol);""",
        """        text.push_str(symbol);
        if amount.minor() < 0 {
            text.push(self.0.minus);
        }""",
    ),
    (
        "locales: a symbol ending in a letter runs into the digits",
        "src/locale.rs",
        "        if spaced {",
        "        if spaced && symbol.is_empty() {",
    ),
    (
        "locales: an ISO code in place of a symbol runs into the digits",
        "src/locale.rs",
        ".map_or((code, true), |symbol| (symbol.text, symbol.spaced))",
        ".map_or((code, false), |symbol| (symbol.text, symbol.spaced))",
    ),
    (
        "locales: the symbol table is searched the wrong way round",
        "src/locale.rs",
        ".binary_search_by(|symbol| symbol.code.cmp(code))",
        ".binary_search_by(|symbol| code.cmp(symbol.code))",
    ),
    (
        "locales: an amount under one unit loses its leading zero",
        "src/locale.rs",
        """let padded = format!("{value:0>width$}", width = decimals.saturating_add(1));""",
        """let padded = format!("{value:0>width$}", width = decimals);""",
    ),
    (
        "locales: groups are separated by the decimal symbol",
        "src/locale.rs",
        """            if index > 0 && left.is_multiple_of(3) {
                text.push(self.0.group);""",
        """            if index > 0 && left.is_multiple_of(3) {
                text.push(self.0.decimal);""",
    ),
    (
        "locales: the most negative amount overflows",
        "src/locale.rs",
        "self.digits(&mut text, amount.minor().unsigned_abs(), usize::from(currency.minor_units()));",
        "self.digits(&mut text, amount.minor().abs().unsigned_abs(), usize::from(currency.minor_units()));",
    ),
    (
        "locales: a whole quantity keeps a decimal",
        "src/locale.rs",
        "while decimals > 0 && value.is_multiple_of(10) {",
        "while decimals > 1 && value.is_multiple_of(10) {",
    ),
    (
        "locales: tags match only in their own case",
        "src/locale.rs",
        ".find(|locale| locale.0.tag.eq_ignore_ascii_case(tag))",
        ".find(|locale| locale.0.tag == tag)",
        # Only shells look a locale up by its tag; no property test has a tag to look up.
        "unit",
    ),
]
