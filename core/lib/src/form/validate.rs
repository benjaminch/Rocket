//! Form field validation routines.
//!
//! Each function in this module can be used as the target of the
//! `field(validate)` field attribute of the `FromForm` derive.
//!
//! ```rust
//! use rocket::form::FromForm;
//!
//! #[derive(FromForm)]
//! struct MyForm<'r> {
//!     #[field(validate = range(2..10))]
//!     id: usize,
//!     #[field(validate = omits("password"))]
//!     password: &'r str,
//! }
//! ```
//!
//! The `validate` parameter takes any expression that returns a
//! [`form::Result<()>`](crate::form::Result). If the expression is a function
//! call, a reference to the field is inserted as the first parameter. Thus,
//! functions calls to `validate` must take a reference to _some_ type,
//! typically a generic with some bounds, as their first argument.
//!
//! ## Custom Error Messages
//!
//! To set a custom error messages, it is useful to chain results:
//!
//! ```rust
//! use rocket::form::{Errors, Error, FromForm};
//!
//! #[derive(FromForm)]
//! struct MyForm<'r> {
//!     // By defining another function...
//!     #[field(validate = omits("password").map_err(pass_help))]
//!     password: &'r str,
//!     // or inline using the `msg` helper.
//!     #[field(validate = omits("password").or_else(msg!("please omit `password`")))]
//!     password2: &'r str,
//!     // You can even refer to the field in the message...
//!     #[field(validate = range(1..).or_else(msg!("`{}` < 1", self.n)))]
//!     n: isize,
//!     // ..or other fields!
//!     #[field(validate = range(..self.n).or_else(msg!("`{}` > `{}`", self.z, self.n)))]
//!     z: isize,
//! }
//!
//! fn pass_help<'a>(errors: Errors<'_>) -> Errors<'a> {
//!     Error::validation("passwords can't contain the text \"password\"").into()
//! }
//! ```
//!
//! ## Custom Validation
//!
//! Custom validation routines can be defined as regular functions. Consider a
//! routine that tries to validate a credit card number:
//!
//! ```rust
//! extern crate time;
//!
//! use rocket::form::{self, FromForm, Error};
//!
//! #[derive(FromForm)]
//! struct CreditCard {
//!     #[field(validate = luhn(self.cvv, &self.expiration))]
//!     number: u64,
//!     cvv: u16,
//!     expiration: time::Date,
//! }
//!
//! // Implementation of Luhn validator.
//! fn luhn<'v>(number: &u64, cvv: u16, exp: &time::Date) -> form::Result<'v, ()> {
//!     # let valid = false;
//!     if !valid {
//!         Err(Error::validation("invalid credit card number"))?;
//!     }
//!
//!     Ok(())
//! }
//! ```

use std::borrow::Cow;
use std::convert::TryInto;
use std::ops::{RangeBounds, Bound};
use std::fmt::Debug;

use crate::data::ByteUnit;
use rocket_http::ContentType;

use crate::{data::TempFile, form::{Result, Error}};

/// A helper macro for custom validation error messages.
///
/// The macro works identically to [`std::format!`] except it does not allocate
/// when the expression is a string literal. It returns a function (a closure)
/// that takes one parameter and evaluates to an `Err` of validation [`Error`]
/// with the formatted message. While useful in other contexts, it is designed
/// to be chained to validation results via `.or_else()` and `.and_then()`.
///
/// Note that the macro never needs to be imported when used with a `FromForm`
/// derive; all items in [`form::validate`](crate::form::validate) are already
/// in scope.
///
/// # Example
///
/// ```rust
/// use rocket::form::FromForm;
///
/// #[derive(FromForm)]
/// struct Person<'r> {
///     #[field(validate = len(3..).or_else(msg!("that's a short name...")))]
///     name: &'r str,
///     #[field(validate = contains('f').and_then(msg!("please, no `f`!")))]
///     non_f_name: &'r str,
/// }
/// ```
///
/// See the [top-level docs](crate::form::validate) for more examples.
#[macro_export]
macro_rules! msg {
    ($e:expr) => (
        |_| {
            Err($crate::form::Errors::from($crate::form::Error::validation($e)))
                as $crate::form::Result<()>
        }
    );
    ($($arg:tt)*) => (
        |_| {
            Err($crate::form::Errors::from($crate::form::Error::validation(format!($($arg)*))))
                as $crate::form::Result<()>
        }
    );
}

#[doc(inline)]
pub use msg;

/// Equality validator: succeeds exactly when `a` == `b`, using [`PartialEq`].
///
/// On failure, returns a validation error with the following message:
///
/// ```text
/// value does not match expected value
/// ```
///
/// # Example
///
/// ```rust
/// use rocket::form::{FromForm, FromFormField};
///
/// #[derive(FromFormField, PartialEq)]
/// enum Kind {
///     Car,
///     Truck
/// }
///
/// #[derive(FromForm)]
/// struct Foo<'r> {
///     #[field(validate = eq("Bob Marley"))]
///     name: &'r str,
///     #[field(validate = eq(Kind::Car))]
///     vehicle: Kind,
///     #[field(validate = eq(&[5, 7, 8]))]
///     numbers: Vec<usize>,
/// }
/// ```
pub fn eq<'v, A, B>(a: &A, b: B) -> Result<'v, ()>
    where A: PartialEq<B>
{
    if a != &b {
        Err(Error::validation("value does not match expected value"))?
    }

    Ok(())
}

/// Negative equality validator: succeeds exactly when `a` != `b`, using
/// [`PartialEq`].
///
/// On failure, returns a validation error with the following message:
///
/// ```text
/// value is equal to an invalid value
/// ```
///
/// # Example
///
/// ```rust
/// use rocket::form::{FromForm, FromFormField};
///
/// #[derive(FromFormField, PartialEq)]
/// enum Kind {
///     Car,
///     Truck
/// }
///
/// #[derive(FromForm)]
/// struct Foo<'r> {
///     #[field(validate = neq("Bob Marley"))]
///     name: &'r str,
///     #[field(validate = neq(Kind::Car))]
///     vehicle: Kind,
///     #[field(validate = neq(&[5, 7, 8]))]
///     numbers: Vec<usize>,
/// }
/// ```
pub fn neq<'v, A, B>(a: &A, b: B) -> Result<'v, ()>
    where A: PartialEq<B>
{
    if a == &b {
        Err(Error::validation("value is equal to an invalid value"))?
    }

    Ok(())
}

/// Types for values that have a length.
///
/// At present, these are:
///
/// | type                              | length description                   |
/// |-----------------------------------|--------------------------------------|
/// | `&str`, `String`                  | length in bytes                      |
/// | `Vec<T>`                          | number of elements in the vector     |
/// | `HashMap<K, V>`, `BTreeMap<K, V>` | number of key/value pairs in the map |
/// | [`TempFile`]                      | length of the file in bytes          |
/// | `Option<T>` where `T: Len`        | length of `T` or 0 if `None`         |
/// | [`form::Result<'_, T>`]           | length of `T` or 0 if `Err`          |
///
/// [`form::Result<'_, T>`]: crate::form::Result
pub trait Len<L> {
    /// The length of the value.
    fn len(&self) -> L;

    /// Convert `len` into `u64`.
    fn len_into_u64(len: L) -> u64;

    /// The zero value for `L`.
    fn zero_len() -> L;
}

macro_rules! impl_len {
    (<$($gen:ident),*> $T:ty => $L:ty) => (
        impl <$($gen),*> Len<$L> for $T {
            fn len(&self) -> $L { self.len() }
            fn len_into_u64(len: $L) -> u64 { len as u64 }
            fn zero_len() -> $L { 0 }
        }
    )
}

impl_len!(<> str => usize);
impl_len!(<> String => usize);
impl_len!(<T> Vec<T> => usize);
impl_len!(<> TempFile<'_> => u64);
impl_len!(<K, V> std::collections::HashMap<K, V> => usize);
impl_len!(<K, V> std::collections::BTreeMap<K, V> => usize);

impl Len<ByteUnit> for TempFile<'_> {
    fn len(&self) -> ByteUnit { self.len().into() }
    fn len_into_u64(len: ByteUnit) -> u64 { len.into() }
    fn zero_len() -> ByteUnit { ByteUnit::from(0) }
}

impl<L, T: Len<L> + ?Sized> Len<L> for &T {
    fn len(&self) -> L { <T as Len<L>>::len(self) }
    fn len_into_u64(len: L) -> u64 { T::len_into_u64(len) }
    fn zero_len() -> L { T::zero_len() }
}

impl<L, T: Len<L>> Len<L> for Option<T> {
    fn len(&self) -> L { self.as_ref().map(|v| v.len()).unwrap_or(T::zero_len()) }
    fn len_into_u64(len: L) -> u64 { T::len_into_u64(len) }
    fn zero_len() -> L { T::zero_len() }
}

impl<L, T: Len<L>> Len<L> for Result<'_, T> {
    fn len(&self) -> L { self.as_ref().ok().len() }
    fn len_into_u64(len: L) -> u64 { T::len_into_u64(len) }
    fn zero_len() -> L { T::zero_len() }
}

/// Length validator: succeeds when the length of a value is within a `range`.
///
/// The value must implement [`Len`]. On failure, returns an [`InvalidLength`]
/// error. See [`Len`] for supported types and how their length is computed.
///
/// [`InvalidLength`]: crate::form::error::ErrorKind::InvalidLength
///
/// # Data Limits
///
/// All form types are constrained by a data limit. As such, the `len()`
/// validator should be used only when a data limit is insufficiently specific.
/// For example, prefer to use data [`Limits`](crate::data::Limits) to validate
/// the length of files as not doing so will result in writing more data to disk
/// than necessary.
///
/// # Example
///
/// ```rust
/// use rocket::http::ContentType;
/// use rocket::form::{FromForm, FromFormField};
/// use rocket::data::{TempFile, ToByteUnit};
///
/// #[derive(FromForm)]
/// struct Foo<'r> {
///     #[field(validate = len(5..20))]
///     name: &'r str,
///     #[field(validate = len(..=200))]
///     maybe_name: Option<&'r str>,
///     #[field(validate = len(..=2.mebibytes()))]
///     #[field(validate = ext(ContentType::Plain))]
///     file: TempFile<'r>,
/// }
/// ```
pub fn len<'v, V, L, R>(value: V, range: R) -> Result<'v, ()>
    where V: Len<L>,
          L: Copy + PartialOrd,
          R: RangeBounds<L>
{
    if !range.contains(&value.len()) {
        let start = match range.start_bound() {
            Bound::Included(v) => Some(V::len_into_u64(*v)),
            Bound::Excluded(v) => Some(V::len_into_u64(*v).saturating_add(1)),
            Bound::Unbounded => None
        };

        let end = match range.end_bound() {
            Bound::Included(v) => Some(V::len_into_u64(*v)),
            Bound::Excluded(v) => Some(V::len_into_u64(*v).saturating_sub(1)),
            Bound::Unbounded => None,
        };

        Err((start, end))?
    }

    Ok(())
}

/// Types for values that contain items.
///
/// At present, these are:
///
/// | type                    | contains                   |
/// |-------------------------|----------------------------|
/// | `&str`, `String`        | `&str`, `char`             |
/// | `Vec<T>`                | `T`, `&T`                  |
/// | `Option<T>`             | `I` where `T: Contains<I>` |
/// | [`form::Result<'_, T>`] | `I` where `T: Contains<I>` |
///
/// [`form::Result<'_, T>`]: crate::form::Result
pub trait Contains<I> {
    /// Returns `true` if `self` contains `item`.
    fn contains(&self, item: I) -> bool;
}

macro_rules! impl_contains {
    ([$($gen:tt)*] $T:ty [contains] $I:ty [via] $P:ty) => {
        impl_contains!([$($gen)*] $T [contains] $I [via] $P [with] |v| v);
    };

    ([$($gen:tt)*] $T:ty [contains] $I:ty [via] $P:ty [with] $f:expr) => {
        impl<$($gen)*> Contains<$I> for $T {
            fn contains(&self, item: $I) -> bool {
                <$P>::contains(self, $f(item))
            }
        }
    };
}

impl_contains!([] str [contains] &str [via] str);
impl_contains!([] str [contains] char [via] str);
impl_contains!([] String [contains] &str [via] str);
impl_contains!([] String [contains] char [via] str);
impl_contains!([T: PartialEq] Vec<T> [contains] &T [via] [T]);

impl<T: PartialEq> Contains<T> for Vec<T> {
    fn contains(&self, item: T) -> bool {
        <[T]>::contains(self, &item)
    }
}

impl<I, T: Contains<I>> Contains<I> for Option<T> {
    fn contains(&self, item: I) -> bool {
        self.as_ref().map(|v| v.contains(item)).unwrap_or(false)
    }
}

impl<I, T: Contains<I>> Contains<I> for Result<'_, T> {
    fn contains(&self, item: I) -> bool {
        self.as_ref().map(|v| v.contains(item)).unwrap_or(false)
    }
}

impl<I, T: Contains<I> + ?Sized> Contains<I> for &T {
    fn contains(&self, item: I) -> bool {
        <T as Contains<I>>::contains(self, item)
    }
}

/// Contains validator: succeeds when a value contains `item`.
///
/// The value must implement [`Contains<I>`](Contains) where `I` is the type of
/// the `item`. See [`Contains`] for supported types and items.
///
/// On failure, returns a validation error with the following message:
///
/// ```text
/// value is equal to an invalid value
/// ```
///
/// # Example
///
/// ```rust
/// use rocket::form::{FromForm, FromFormField};
///
/// #[derive(PartialEq, FromFormField)]
/// enum Pet { Cat, Dog }
///
/// #[derive(FromForm)]
/// struct Foo<'r> {
///     best_pet: Pet,
///     #[field(validate = contains(Pet::Cat))]
///     #[field(validate = contains(&self.best_pet))]
///     pets: Vec<Pet>,
///     #[field(validate = contains('/'))]
///     license: &'r str,
///     #[field(validate = contains("@rust-lang.org"))]
///     rust_lang_email: &'r str,
/// }
/// ```
pub fn contains<'v, V, I>(value: V, item: I) -> Result<'v, ()>
    where V: Contains<I>
{
    if !value.contains(item) {
        Err(Error::validation("value does not contain expected item"))?
    }

    Ok(())
}

/// Verbose contains validator: like `contains` but mentions `item` in the
/// error message.
///
/// The is identical to [`contains()`] except that `item` must be `Debug + Copy`
/// and the error message is as follows, where `$item` is the [`Debug`]
/// representation of `item`:
///
/// ```text
/// values must contains $item
/// ```
///
/// # Example
///
/// ```rust
/// use rocket::form::{FromForm, FromFormField};
///
/// #[derive(PartialEq, Debug, Clone, Copy, FromFormField)]
/// enum Pet { Cat, Dog }
///
/// #[derive(FromForm)]
/// struct Foo {
///     best_pet: Pet,
///     #[field(validate = verbose_contains(Pet::Dog))]
///     #[field(validate = verbose_contains(&self.best_pet))]
///     pets: Vec<Pet>,
/// }
/// ```
pub fn verbose_contains<'v, V, I>(value: V, item: I) -> Result<'v, ()>
    where V: Contains<I>, I: Debug + Copy
{
    if !value.contains(item) {
        Err(Error::validation(format!("value must contain {:?}", item)))?
    }

    Ok(())
}

/// Omits validator: succeeds when a value _does not_ contains `item`.
/// error message.
///
/// The value must implement [`Contains<I>`](Contains) where `I` is the type of
/// the `item`. See [`Contains`] for supported types and items.
///
/// On failure, returns a validation error with the following message:
///
/// ```text
/// value contains a disallowed item
/// ```
///
/// # Example
///
/// ```rust
/// use rocket::form::{FromForm, FromFormField};
///
/// #[derive(PartialEq, FromFormField)]
/// enum Pet { Cat, Dog }
///
/// #[derive(FromForm)]
/// struct Foo<'r> {
///     #[field(validate = omits(Pet::Cat))]
///     pets: Vec<Pet>,
///     #[field(validate = omits('@'))]
///     not_email: &'r str,
///     #[field(validate = omits("@gmail.com"))]
///     non_gmail_email: &'r str,
/// }
/// ```
pub fn omits<'v, V, I>(value: V, item: I) -> Result<'v, ()>
    where V: Contains<I>
{
    if value.contains(item) {
        Err(Error::validation("value contains a disallowed item"))?
    }

    Ok(())
}

/// Verbose omits validator: like `omits` but mentions `item` in the error
/// message.
///
/// The is identical to [`omits()`] except that `item` must be `Debug + Copy`
/// and the error message is as follows, where `$item` is the [`Debug`]
/// representation of `item`:
///
/// ```text
/// value cannot contain $item
/// ```
///
/// # Example
///
/// ```rust
/// use rocket::form::{FromForm, FromFormField};
///
/// #[derive(PartialEq, Debug, Clone, Copy, FromFormField)]
/// enum Pet { Cat, Dog }
///
/// #[derive(FromForm)]
/// struct Foo<'r> {
///     #[field(validate = verbose_omits(Pet::Cat))]
///     pets: Vec<Pet>,
///     #[field(validate = verbose_omits('@'))]
///     not_email: &'r str,
///     #[field(validate = verbose_omits("@gmail.com"))]
///     non_gmail_email: &'r str,
/// }
/// ```
pub fn verbose_omits<'v, V, I>(value: V, item: I) -> Result<'v, ()>
    where V: Contains<I>, I: Copy + Debug
{
    if value.contains(item) {
        Err(Error::validation(format!("value cannot contain {:?}", item)))?
    }

    Ok(())
}

/// Integer range validator: succeeds when an integer value is within a range.
///
/// The value must be an integer type that implement `TryInto<isize> + Copy`. On
/// failure, returns an [`OutOfRange`] error.
///
/// [`OutOfRange`]: crate::form::error::ErrorKind::OutOfRange
///
/// # Example
///
/// ```rust
/// use rocket::form::FromForm;
///
/// #[derive(FromForm)]
/// struct Foo {
///     #[field(validate = range(0..))]
///     non_negative: isize,
///     #[field(validate = range(18..=130))]
///     maybe_adult: u8,
/// }
/// ```
pub fn range<'v, V, R>(value: &V, range: R) -> Result<'v, ()>
    where V: TryInto<isize> + Copy, R: RangeBounds<isize>
{
    if let Ok(v) = (*value).try_into() {
        if range.contains(&v) {
            return Ok(());
        }
    }

    let start = match range.start_bound() {
        Bound::Included(v) => Some(*v),
        Bound::Excluded(v) => Some(v.saturating_add(1)),
        Bound::Unbounded => None
    };

    let end = match range.end_bound() {
        Bound::Included(v) => Some(*v),
        Bound::Excluded(v) => Some(v.saturating_sub(1)),
        Bound::Unbounded => None,
    };


    Err((start, end))?
}

/// Contains one of validator: succeeds when a value contains at least one item
/// in an `items` iterator.
///
/// The value must implement [`Contains<I>`](Contains) where `I` is the type of
/// the `item`. The iterator must be [`Clone`]. See [`Contains`] for supported
/// types and items. The item must be [`Debug`].
///
/// On failure, returns a [`InvalidChoice`] error with the debug representation
/// of each item in `items`.
///
/// [`InvalidChoice`]: crate::form::error::ErrorKind::InvalidChoice
///
/// # Example
///
/// ```rust
/// use rocket::form::FromForm;
///
/// #[derive(FromForm)]
/// struct Foo<'r> {
///     #[field(validate = one_of(&[3, 5, 7]))]
///     single_digit_primes: Vec<u8>,
///     #[field(validate = one_of(" \t\n".chars()))]
///     has_space_char: &'r str,
///     #[field(validate = one_of(" \t\n".chars()).and_then(msg!("no spaces")))]
///     no_space: &'r str,
/// }
/// ```
pub fn one_of<'v, V, I, R>(value: V, items: R) -> Result<'v, ()>
    where V: Contains<I>,
          I: Debug,
          R: IntoIterator<Item = I>,
          <R as IntoIterator>::IntoIter: Clone
{
    let items = items.into_iter();
    for item in items.clone() {
        if value.contains(item) {
            return Ok(());
        }
    }

    let choices: Vec<Cow<'_, str>> = items
        .map(|item| format!("{:?}", item).into())
        .collect();

    Err(choices)?
}

/// File type validator: succeeds when a [`TempFile`] has the Content-Type
/// `content_type`.
///
/// On failure, returns a validation error with one of the following messages:
///
/// ```text
/// // the file has an incorrect extension
/// file type was .$file_ext but must be $type
///
/// // the file does not have an extension
/// file type must be $type
/// ```
///
/// # Example
///
/// ```rust
/// use rocket::form::FromForm;
/// use rocket::data::{ToByteUnit, TempFile};
/// use rocket::http::ContentType;
///
/// #[derive(FromForm)]
/// struct Foo<'r> {
///     #[field(validate = ext(ContentType::PDF))]
///     #[field(validate = len(..1.mebibytes()))]
///     document: TempFile<'r>,
/// }
/// ```
pub fn ext<'v>(file: &TempFile<'_>, r#type: ContentType) -> Result<'v, ()> {
    if let Some(file_ct) = file.content_type() {
        if file_ct == &r#type {
            return Ok(());
        }
    }

    let msg = match (file.content_type().and_then(|c| c.extension()), r#type.extension()) {
        (Some(a), Some(b)) => format!("invalid file type: .{}, must be .{}", a, b),
        (Some(a), None) => format!("invalid file type: .{}, must be {}", a, r#type),
        (None, Some(b)) => format!("file type must be .{}", b),
        (None, None) => format!("file type must be {}", r#type),
    };

    Err(Error::validation(msg))?
}
