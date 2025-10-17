//! ForEach iteration helper

#![allow(non_snake_case)]

use crate::composable;
use std::hash::Hash;

#[composable(no_skip)]
pub fn ForEach<T, F>(items: &[T], mut row: F)
where
    T: Hash,
    F: FnMut(&T) + 'static,
{
    for item in items {
        compose_core::with_key(item, || row(item));
    }
}
