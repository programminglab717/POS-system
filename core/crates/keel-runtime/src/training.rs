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
        "ENTRENAMIENTO - NO ES UN RECIBO"
    } else {
        "TRAINING - NOT A RECEIPT"
    }
}

/// `doc` as a register prints it: as it is, or, `training`, with [`banner`] above and below and
/// no drawer's kick. A document that can't take the banner, at a document's limits, prints the
/// banner alone: never as if it were real.
pub(crate) fn printed(doc: Doc, training: bool, locale: Locale) -> Doc {
    if !training {
        return doc;
    }
    let mark = || {
        Block::text(banner(locale), Align::Center, Style { bold: true, wide: false, tall: true })
    };
    let kept: Vec<Block> =
        doc.blocks().iter().filter(|block| !matches!(block, Block::DrawerKick)).cloned().collect();
    let alone = || Doc::new(vec![mark(), Block::Feed(2), Block::Cut]);
    let with = |blocks: Vec<Block>| -> Option<Doc> {
        let mut all = vec![mark(), Block::Rule];
        all.extend(blocks);
        all.push(mark());
        Doc::new(all).ok()
    };
    with(kept).or_else(|| alone().ok()).unwrap_or(doc_without_kick(doc))
}

/// `doc` with no drawer's kick: what's left when even the banner can't be made, which a
/// document of one block always can.
fn doc_without_kick(doc: Doc) -> Doc {
    let kept: Vec<Block> =
        doc.blocks().iter().filter(|block| !matches!(block, Block::DrawerKick)).cloned().collect();
    Doc::new(kept).unwrap_or(doc)
}

impl<S: Signer, E: Entropy, C: Clock, I: Entropy> Runtime<S, E, C, I> {
    /// Makes this a training register (ADR-0038), for good: it never syncs, and what it prints
    /// says so. Called on a register over a store of its own, before anything else.
    ///
    /// # Errors
    /// [`RuntimeError::Training`] if it is syncing already.
    pub fn start_training(&mut self) -> Result<(), RuntimeError> {
        if self.sync.is_some() {
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
