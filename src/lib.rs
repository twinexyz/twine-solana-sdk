pub mod account;
pub mod atomic_u64;
pub mod clock;
pub mod decode_error;
pub mod hash;
pub mod pubkey;
pub mod sanitize;

/// The unit of time a given leader schedule is honored.
///
/// It lasts for some number of [`Slot`]s.
pub type Epoch = u64;
