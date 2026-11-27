use std::ops::Deref;

use syntax::types::Type;

use super::NormalizationContext;

pub(super) enum Position<'c, 'a> {
    Interface(Type),
    Value(ValueContext<'c, 'a>),
}

pub(super) struct ValueContext<'c, 'a> {
    context: &'c NormalizationContext<'a>,
}

impl<'a> Deref for ValueContext<'_, 'a> {
    type Target = NormalizationContext<'a>;

    fn deref(&self) -> &Self::Target {
        self.context
    }
}

impl<'a> NormalizationContext<'a> {
    pub(super) fn position(&self) -> Position<'_, 'a> {
        let interface = self
            .scrutinee_type
            .as_ref()
            .map(|ty| self.store.peel_alias(ty))
            .filter(|ty| self.store.is_interface(ty));
        match interface {
            Some(interface) => Position::Interface(interface),
            None => Position::Value(ValueContext { context: self }),
        }
    }
}
