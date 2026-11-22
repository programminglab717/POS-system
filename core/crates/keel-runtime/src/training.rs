//! Training (ADR-0038): a register over a store of its own, which the shell opens apart from the
//! real one. It never syncs, so nothing it records reaches the hub, the cloud, a report or an
//! export; everything it prints carries a banner above and below, and no drawer is kicked.

use keel_doc::{Align, Block, Doc, Style};
use keel_events::keys::Signer;
use keel_types::{Clock, Entropy, Locale};

use crate::error::RuntimeError;
use crate::runtime::Runtime;

/// The banner a training register's documents carry, in `locale`'s language.
pub fn banner(locale: Locale) -> &'static str {
    if locale.tag().starts_with("es") {
        "PRÁCTICA - NO ES UN RECIBO"
    } else {
        "TRAINING - NOT A RECEIPT"
    }
}

/// The banner a training register's kitchen tickets carry: a cook reads "not a receipt" as
/// nothing to do with them (ADR-0038). Found in review.
pub fn kitchen_banner(locale: Locale) -> &'static str {
    if locale.tag().starts_with("es") {
        "PRÁCTICA - NO PREPARAR"
    } else {
        "TRAINING - DO NOT MAKE"
    }
}

/// `doc` as a register prints it: as it is, or, `training`, with `banner` above it and below
/// it, above the feed and cut that end it, so the slip torn off carries both: found in review,
/// the lower banner landed on the next slip. No drawer's kick. A document that can't take the
/// banners, at a document's limits, prints the banner alone: never as if it were real.
pub(crate) fn printed(doc: Doc, training: bool, banner: &str) -> Doc {
    if !training {
        return doc;
    }
    let mark = || Block::text(banner, Align::Center, Style { bold: true, wide: false, tall: true });
    let mut kept: Vec<Block> =
        doc.blocks().iter().filter(|block| !matches!(block, Block::DrawerKick)).cloned().collect();
    // The feed and cut that end it go after the lower banner.
    let mut end = Vec::new();
    while matches!(kept.last(), Some(Block::Feed(_) | Block::Cut)) {
        if let Some(block) = kept.pop() {
            end.insert(0, block);
        }
    }
    let mut all = vec![mark(), Block::Rule];
    all.extend(kept);
    all.push(Block::Rule);
    all.push(mark());
    all.extend(end);
    Doc::new(all).or_else(|_| Doc::new(vec![mark(), Block::Feed(2), Block::Cut])).unwrap_or(doc)
}

impl<S: Signer, E: Entropy, C: Clock, I: Entropy> Runtime<S, E, C, I> {
    /// Makes this a training register (ADR-0038), for good: it never syncs, and what it prints
    /// says so. Called on a register over a new store of its own, before anything else; the
    /// shell gives it an identity of its own too, which no location's roster has.
    ///
    /// # Errors
    /// [`RuntimeError::Training`] if it is syncing already, or its store holds anything.
    pub fn start_training(&mut self) -> Result<(), RuntimeError> {
        // Only over a store that holds nothing: a real store opened for training would put a
        // trainee's sales in the real log, to sync at its next real open. Found in review.
        let empty =
            self.store.head(self.device)?.seq() == 0 && self.store.clock() == keel_types::Hlc::ZERO;
        if self.sync.is_some() || !empty {
            return Err(RuntimeError::Training);
        }
        self.training = true;
        Ok(())
    }

    /// Whether this is a training register.
    pub const fn is_training(&self) -> bool {
        self.training
    }

    /// The language the register speaks to the member signed in: theirs, or else the
    /// register's (ADR-0038). `None` when no one is signed in.
    pub fn language(&self) -> Option<Locale> {
        let member = self.profile.member(self.member?)?;
        Some(member.language.unwrap_or(self.locale))
    }
}

#[cfg(test)]
mod tests {
    use keel_doc::{Align, Block, Doc, Style};

    use super::printed;

    /// Both banners above the feed and cut that end a document, and no kick: found in review,
    /// the lower banner printed after the cut, on the next slip.
    #[test]
    fn the_banners_are_above_the_feed_and_cut_and_the_kick_is_gone() {
        let line = || Block::text("Thank you!", Align::Center, Style::default());
        let doc = Doc::new(vec![Block::DrawerKick, line(), Block::Feed(2), Block::Cut]).unwrap();
        let out = printed(doc.clone(), true, "TRAINING");
        let blocks = out.blocks();
        assert!(!blocks.contains(&Block::DrawerKick));
        let banner =
            |block: &Block| matches!(block, Block::Text { text, .. } if text == "TRAINING");
        assert!(banner(&blocks[0]), "{blocks:?}");
        assert_eq!(blocks[blocks.len() - 2..], [Block::Feed(2), Block::Cut], "{blocks:?}");
        assert!(
            banner(&blocks[blocks.len() - 3]),
            "the lower banner just above the feed: {blocks:?}"
        );
        assert_eq!(printed(doc.clone(), false, "TRAINING"), doc, "a real register's, as it is");
    }
}
