use std::iter;

#[derive(Clone)]
pub(crate) struct ScopeStack<T> {
    base: T,
    nested: Vec<T>,
}

impl<T> ScopeStack<T> {
    pub(crate) fn new(base: T) -> Self {
        Self {
            base,
            nested: Vec::new(),
        }
    }

    pub(crate) fn innermost(&self) -> &T {
        self.nested.last().unwrap_or(&self.base)
    }

    pub(crate) fn innermost_mut(&mut self) -> &mut T {
        self.nested.last_mut().unwrap_or(&mut self.base)
    }

    pub(crate) fn push(&mut self, frame: T) {
        self.nested.push(frame);
    }

    pub(crate) fn pop(&mut self) -> Option<T> {
        self.nested.pop()
    }

    pub(crate) fn iter(&self) -> impl DoubleEndedIterator<Item = &T> {
        iter::once(&self.base).chain(&self.nested)
    }

    pub(crate) fn iter_mut(&mut self) -> impl DoubleEndedIterator<Item = &mut T> {
        iter::once(&mut self.base).chain(&mut self.nested)
    }

    pub(crate) fn enclosing(&self) -> impl Iterator<Item = &T> {
        self.iter().take(self.nested.len())
    }
}
