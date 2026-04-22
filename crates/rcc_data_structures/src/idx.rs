//! Typed indices backed by `u32` + a `Vec<T>` indexed by them.
//!
//! Similar to `rustc_index::IndexVec`. Using a typed index prevents accidental
//! cross-use of `NodeId`/`HirId`/`DefId`/`Local`/`BasicBlockId`.

use std::marker::PhantomData;

/// Trait for newtype indices over `u32`.
pub trait Idx: Copy + Eq + std::hash::Hash + std::fmt::Debug {
    /// Build from a `usize`.
    fn new(idx: usize) -> Self;
    /// Extract as `usize`.
    fn index(self) -> usize;
}

/// Vec indexed by a typed `Idx`.
#[derive(Debug, Clone)]
pub struct IndexVec<I: Idx, T> {
    raw: Vec<T>,
    _marker: PhantomData<fn(I) -> I>,
}

impl<I: Idx, T> Default for IndexVec<I, T> {
    fn default() -> Self {
        Self { raw: Vec::new(), _marker: PhantomData }
    }
}

impl<I: Idx, T> IndexVec<I, T> {
    /// Empty vec.
    pub fn new() -> Self {
        Self::default()
    }

    /// Vec preallocated for `cap` elements.
    pub fn with_capacity(cap: usize) -> Self {
        Self { raw: Vec::with_capacity(cap), _marker: PhantomData }
    }

    /// Push and return the new index.
    pub fn push(&mut self, value: T) -> I {
        let idx = I::new(self.raw.len());
        self.raw.push(value);
        idx
    }

    /// Number of elements.
    pub fn len(&self) -> usize {
        self.raw.len()
    }

    /// Whether no elements are stored.
    pub fn is_empty(&self) -> bool {
        self.raw.is_empty()
    }

    /// Borrow element at `i`.
    pub fn get(&self, i: I) -> Option<&T> {
        self.raw.get(i.index())
    }

    /// Mutable borrow of element at `i`.
    pub fn get_mut(&mut self, i: I) -> Option<&mut T> {
        self.raw.get_mut(i.index())
    }

    /// Iterator over `(I, &T)` pairs.
    pub fn iter_enumerated(&self) -> impl Iterator<Item = (I, &T)> {
        self.raw.iter().enumerate().map(|(i, v)| (I::new(i), v))
    }

    /// Iterator over values.
    pub fn iter(&self) -> std::slice::Iter<'_, T> {
        self.raw.iter()
    }

    /// Mutable iterator.
    pub fn iter_mut(&mut self) -> std::slice::IterMut<'_, T> {
        self.raw.iter_mut()
    }

    /// Backing `Vec`.
    pub fn raw(&self) -> &[T] {
        &self.raw
    }
}

impl<I: Idx, T> std::ops::Index<I> for IndexVec<I, T> {
    type Output = T;
    fn index(&self, i: I) -> &T {
        &self.raw[i.index()]
    }
}

impl<I: Idx, T> std::ops::IndexMut<I> for IndexVec<I, T> {
    fn index_mut(&mut self, i: I) -> &mut T {
        &mut self.raw[i.index()]
    }
}

/// Declare a `u32`-backed newtype implementing [`Idx`].
///
/// ```ignore
/// rcc_data_structures::new_index! {
///     pub struct BasicBlockId = u32;
/// }
/// ```
#[macro_export]
macro_rules! new_index {
    ($(#[$m:meta])* $vis:vis struct $name:ident = u32;) => {
        $(#[$m])*
        #[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
        $vis struct $name(pub u32);

        impl $crate::idx::Idx for $name {
            #[inline]
            fn new(idx: usize) -> Self { Self(idx as u32) }
            #[inline]
            fn index(self) -> usize { self.0 as usize }
        }
    };
}
