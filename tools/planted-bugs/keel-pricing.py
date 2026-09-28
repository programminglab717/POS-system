"""Planted bugs for keel-pricing: see run.py."""

BUGS = [
    # Extension.
    (
        "extension rounds with the discount mode",
        "src/engine.rs",
        "let whole = unit_price.mul_decimal(line.quantity.to_decimal(), rules.extension)?;",
        "let whole = unit_price.mul_decimal(line.quantity.to_decimal(), rules.discounts)?;",
    ),
    (
        "extension always truncates",
        "src/engine.rs",
        "let whole = unit_price.mul_decimal(line.quantity.to_decimal(), rules.extension)?;",
        "let whole = unit_price.mul_decimal(line.quantity.to_decimal(), RoundingMode::TowardZero)?;",
    ),
    (
        "modifier quantities ignored",
        "src/engine.rs",
        "let price = each.checked_mul(i64::from(modifier.quantity.get()))?;",
        "let price = each;",
    ),
    (
        "nested modifiers ignored",
        "src/engine.rs",
        """let each =
            modifier.unit_price.checked_add(modifiers_price(&modifier.modifiers, currency)?)?;""",
        "let each = modifier.unit_price;",
    ),
    (
        "modifiers counted once per line, not per unit",
        "src/engine.rs",
        "let whole = unit_price.mul_decimal(line.quantity.to_decimal(), rules.extension)?;",
        "let whole = line.unit_price.mul_decimal(line.quantity.to_decimal(), rules.extension)?.checked_add(modifiers)?;",
    ),
    # Shares of a line (ADR-0015).
    (
        "shares take the whole line",
        "src/engine.rs",
        "let part = part_of(whole, share, index)?;",
        "let part = whole;",
    ),
    (
        "shares always take the first part",
        "src/engine.rs",
        "parts.get(share.index).copied().ok_or(PricingError::InvalidShare { line })",
        "parts.first().copied().ok_or(PricingError::InvalidShare { line })",
    ),
    (
        "shares give the spare units to the last parts",
        "src/engine.rs",
        """    let parts = whole.allocate(&share.weights)?;
    parts.get(share.index).copied().ok_or(PricingError::InvalidShare { line })""",
        """    let mut weights = share.weights.clone();
    weights.reverse();
    let parts = whole.allocate(&weights)?;
    let from_end = parts.len().checked_sub(1).and_then(|last| last.checked_sub(share.index));
    from_end.and_then(|at| parts.get(at)).copied().ok_or(PricingError::InvalidShare { line })""",
    ),
    (
        "zero weights accepted",
        "src/engine.rs",
        "share.index < share.weights.len() && share.weights.iter().all(|&weight| weight > 0)",
        "share.index < share.weights.len()",
    ),
    (
        "a comp takes the whole shared line",
        "src/engine.rs",
        "let (comp, mut net) = if line.comped { (gross, zero) } else { (zero, gross) };",
        "let (comp, mut net) = if line.comped { (whole, zero) } else { (zero, gross) };",
    ),
    (
        "shares leave no trace",
        "src/engine.rs",
        """            trace.push(Step::Shared {
                line: index,
                whole,
                weights: share.weights.clone(),
                index: share.index,
                part,
            });
""",
        "",
    ),
    # Comps and line discounts.
    (
        "comped lines keep their price",
        "src/engine.rs",
        "let (comp, mut net) = if line.comped { (gross, zero) } else { (zero, gross) };",
        "let (comp, mut net) = if line.comped { (zero, gross) } else { (zero, gross) };",
    ),
    (
        "line discounts all taken from the gross",
        "src/engine.rs",
        "let taken = take(net, discount, rules.discounts)?;",
        "let taken = take(gross, discount, rules.discounts)?;",
    ),
    (
        "amount discounts not limited to what is left",
        "src/engine.rs",
        "Ok(Taken { amount: if limited { rest } else { amount }, limited })",
        "Ok(Taken { amount, limited })",
    ),
    (
        "percentage discounts always truncated",
        "src/engine.rs",
        "Ok(Taken { amount: rest.mul_decimal(rate.as_fraction(), mode)?, limited: false })",
        "Ok(Taken { amount: rest.mul_decimal(rate.as_fraction(), RoundingMode::TowardZero)?, limited: false })",
    ),
    (
        "amount discounts limited when equal to what is left",
        "src/engine.rs",
        "let limited = amount.compare(rest)? == Ordering::Greater;",
        "let limited = amount.compare(rest)? != Ordering::Less;",
    ),
    # Order discounts.
    (
        "order discounts allocated by gross",
        "src/engine.rs",
        "let shares = allocate(taking.amount, &rests)?;",
        "let shares = allocate(taking.amount, &lines.iter().map(|line| line.gross).collect::<Vec<_>>())?;",
    ),
    (
        "order discounts taken from the gross subtotal",
        "src/engine.rs",
        "let base = Money::sum(basket.currency, rests.iter().copied())?;",
        "let base = Money::sum(basket.currency, lines.iter().map(|line| line.gross))?;",
    ),
    (
        "order discount shares not taken from the lines",
        "src/engine.rs",
        "line.net = line.net.checked_sub(share)?;",
        "let _ = share;",
    ),
    (
        "order discount shares recorded on the wrong line",
        "src/engine.rs",
        "for (line, &share) in lines.iter_mut().zip(&shares) {",
        "for (line, &share) in lines.iter_mut().rev().zip(&shares) {",
    ),
    # Tax.
    (
        "tax computed on the gross",
        "src/engine.rs",
        ".map(|line| tax.applies(line.category, basket.dining).then_some(line.net))",
        ".map(|line| tax.applies(line.category, basket.dining).then_some(line.gross))",
    ),
    (
        "dining conditions ignored",
        "src/rules.rs",
        "self.categories.contains(&category) && self.dining.is_none_or(|only| only == dining)",
        "self.categories.contains(&category)",
    ),
    (
        "dining conditions inverted",
        "src/rules.rs",
        "self.dining.is_none_or(|only| only == dining)",
        "self.dining.is_none_or(|only| only != dining)",
    ),
    (
        "exemptions ignored",
        "src/engine.rs",
        "if basket.exemptions.contains(&tax.id) {",
        "if false {",
    ),
    (
        "exemptions leave no trace",
        "src/engine.rs",
        """            trace.push(Step::Exempted { tax: position });
""",
        "",
    ),
    (
        "document taxes on nothing leave no trace",
        "src/engine.rs",
        """                trace.push(Step::TaxedDocument {
                    tax: position,
                    rate,
                    taxable: base,
                    mode,
                    amount,
                    shares: shares.clone(),
                });
""",
        """                if !base.is_zero() {
                    trace.push(Step::TaxedDocument {
                        tax: position,
                        rate,
                        taxable: base,
                        mode,
                        amount,
                        shares: shares.clone(),
                    });
                }
""",
    ),
    (
        "empty baskets list no taxes",
        "src/engine.rs",
        """    let zero = Money::zero(basket.currency);
    let TaxRounding { scope, mode } = rules.tax_rounding;""",
        """    if lines.is_empty() {
        return Ok(Vec::new());
    }
    let zero = Money::zero(basket.currency);
    let TaxRounding { scope, mode } = rules.tax_rounding;""",
    ),
    (
        "tax always rounded per line",
        "src/engine.rs",
        "let TaxRounding { scope, mode } = rules.tax_rounding;",
        """let TaxRounding { scope: _, mode } = rules.tax_rounding;
    let scope = TaxScope::Line;""",
    ),
    (
        "tax always rounded per document",
        "src/engine.rs",
        "let TaxRounding { scope, mode } = rules.tax_rounding;",
        """let TaxRounding { scope: _, mode } = rules.tax_rounding;
    let scope = TaxScope::Document;""",
    ),
    (
        "tax rounded with the extension mode",
        "src/engine.rs",
        "let TaxRounding { scope, mode } = rules.tax_rounding;",
        """let TaxRounding { scope, mode: _ } = rules.tax_rounding;
    let mode = rules.extension;""",
    ),
    (
        "document tax shared with uncovered lines",
        "src/engine.rs",
        "let weights: Vec<Money> = taxable.iter().map(|part| part.unwrap_or(zero)).collect();",
        "let weights: Vec<Money> = lines.iter().map(|line| line.net).collect();",
    ),
    (
        "document tax total counted from the shares' first line only",
        "src/engine.rs",
        "let total = Money::sum(basket.currency, amounts.iter().flatten().copied())?;",
        "let total = Money::sum(basket.currency, amounts.iter().flatten().copied().take(1))?;",
    ),
    # Totals.
    (
        "discounted counts only order discounts",
        "src/engine.rs",
        "let discounted = gross.checked_sub(net)?;",
        "let discounted = Money::sum(currency, discounts.iter().copied())?;",
    ),
    (
        "line totals leave out tax",
        "src/engine.rs",
        "let total = work.net.checked_add(tax)?;",
        "let total = work.net;",
    ),
    (
        "order total leaves out tax",
        "src/engine.rs",
        "let total = net.checked_add(tax)?;",
        "let total = net;",
    ),
    # Validation.
    (
        "negative prices accepted",
        "src/engine.rs",
        "if line.unit_price.is_negative() {",
        "if false {",
    ),
    (
        "negative modifier prices accepted",
        "src/engine.rs",
        "if modifier.unit_price.is_negative() {",
        "if false {",
    ),
    (
        "zero quantities accepted",
        "src/engine.rs",
        "if !line.quantity.is_positive() {",
        "if line.quantity.is_negative() {",
    ),
    (
        "percentages above 100% accepted",
        "src/engine.rs",
        "Discount::Percent(rate) => (Decimal::ZERO..=Decimal::ONE).contains(&rate.as_fraction()),",
        "Discount::Percent(rate) => Decimal::ZERO <= rate.as_fraction(),",
    ),
    (
        "negative percentages accepted",
        "src/engine.rs",
        "Discount::Percent(rate) => (Decimal::ZERO..=Decimal::ONE).contains(&rate.as_fraction()),",
        "Discount::Percent(rate) => rate.as_fraction() <= Decimal::ONE,",
    ),
    (
        "negative amount discounts accepted",
        "src/engine.rs",
        """            !amount.is_negative()
""",
        """            true
""",
    ),
    (
        "discount currencies unchecked",
        "src/engine.rs",
        """            same_currency(amount)?;
            !amount.is_negative()""",
        "            !amount.is_negative()",
    ),
    (
        "line currencies unchecked",
        "src/engine.rs",
        """        same_currency(line.unit_price)?;
""",
        "",
    ),
    (
        "negative tax rates accepted",
        "src/engine.rs",
        "if tax.rate.as_fraction() < Decimal::ZERO {",
        "if false {",
    ),
    (
        "tax rates above 100% refused",
        "src/engine.rs",
        "if tax.rate.as_fraction() < Decimal::ZERO {",
        "if !(Decimal::ZERO..=Decimal::ONE).contains(&tax.rate.as_fraction()) {",
    ),
    (
        "repeated taxes accepted",
        "src/engine.rs",
        "if earlier.iter().any(|other| other.id == tax.id) {",
        "if false {",
    ),
    (
        "modifier depth unchecked",
        "src/engine.rs",
        "if depth > MAX_MODIFIER_DEPTH {",
        "if false {",
    ),
    (
        "modifier depth off by one",
        "src/engine.rs",
        "if depth > MAX_MODIFIER_DEPTH {",
        "if depth >= MAX_MODIFIER_DEPTH {",
    ),
    # The trace.
    (
        "trace records the gross as a line discount's base",
        "src/engine.rs",
        """            base: net,
""",
        """            base: gross,
""",
    ),
    (
        "trace records the extension mode wrongly",
        "src/engine.rs",
        """        mode: rules.extension,
        gross: whole,""",
        """        mode: rules.discounts,
        gross: whole,""",
    ),
    # Cash rounding.
    (
        "cash rounding's sign reversed",
        "src/cash.rs",
        "let rounding = due.checked_sub(amount)?;",
        "let rounding = amount.checked_sub(due)?;",
    ),
]
