//! The Solana [`Account`] type.

pub mod account_hasher;
pub mod accounts_db;
pub mod debug_account_data;

use core::cell::{Ref, RefCell};
use core::mem::MaybeUninit;
use core::{fmt, ptr};
use std::rc::Rc;
use std::sync::Arc;

use debug_account_data::debug_account_data;
use serde::{Deserialize, Serialize, Serializer};

use crate::clock::Epoch;
use crate::lamports::LamportsError;
use crate::pubkey::Pubkey;

/// An Account with data that is stored on chain
#[repr(C)]
#[derive(Deserialize, PartialEq, Eq, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct Account {
    /// lamports in the account
    pub lamports: u64,
    /// data held in this account
    #[serde(with = "serde_bytes")]
    pub data: Vec<u8>,
    /// the program that owns this account. If executable, the program that loads this account.
    pub owner: Pubkey,
    /// this account's data contains a loaded program (and is now read-only)
    pub executable: bool,
    /// the epoch at which this account will next owe rent
    pub rent_epoch: Epoch,
}

// mod because we need 'Account' below to have the name 'Account' to match expected serialization
mod account_serialize {
    use serde::ser::Serializer;
    use serde::Serialize;

    use crate::account::ReadableAccount;
    use crate::clock::Epoch;
    use crate::pubkey::Pubkey;
    #[repr(C)]
    #[derive(Serialize)]
    #[serde(rename_all = "camelCase")]
    struct Account<'a> {
        lamports: u64,
        #[serde(with = "serde_bytes")]
        // a slice so we don't have to make a copy just to serialize this
        data: &'a [u8],
        // can't be &pubkey because abi example doesn't support it
        owner: Pubkey,
        executable: bool,
        rent_epoch: Epoch,
    }

    /// allows us to implement serialize on AccountSharedData that is equivalent to Account::serialize without making a copy of the Vec<u8>
    pub fn serialize_account<S>(
        account: &(impl ReadableAccount + Serialize),
        serializer: S,
    ) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let temp = Account {
            lamports: account.lamports(),
            data: account.data(),
            owner: *account.owner(),
            executable: account.executable(),
            rent_epoch: account.rent_epoch(),
        };
        temp.serialize(serializer)
    }
}

impl Serialize for Account {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        crate::account::account_serialize::serialize_account(self, serializer)
    }
}

impl Serialize for AccountSharedData {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        crate::account::account_serialize::serialize_account(self, serializer)
    }
}

/// An Account with data that is stored on chain
/// This will be the in-memory representation of the 'Account' struct data.
/// The existing 'Account' structure cannot easily change due to downstream projects.
#[derive(PartialEq, Eq, Clone, Default, Deserialize)]
#[serde(from = "Account")]
pub struct AccountSharedData {
    /// lamports in the account
    lamports: u64,
    /// data held in this account
    data: Arc<Vec<u8>>,
    /// the program that owns this account. If executable, the program that loads this account.
    owner: Pubkey,
    /// this account's data contains a loaded program (and is now read-only)
    executable: bool,
    /// the epoch at which this account will next owe rent
    rent_epoch: Epoch,
}

/// Compares two ReadableAccounts
///
/// Returns true if accounts are essentially equivalent as in all fields are equivalent.
pub fn accounts_equal<T: ReadableAccount, U: ReadableAccount>(me: &T, other: &U) -> bool {
    me.lamports() == other.lamports()
        && me.executable() == other.executable()
        && me.rent_epoch() == other.rent_epoch()
        && me.owner() == other.owner()
        && me.data() == other.data()
}

impl From<AccountSharedData> for Account {
    fn from(mut other: AccountSharedData) -> Self {
        let account_data = Arc::make_mut(&mut other.data);
        Self {
            lamports: other.lamports,
            data: std::mem::take(account_data),
            owner: other.owner,
            executable: other.executable,
            rent_epoch: other.rent_epoch,
        }
    }
}

impl From<Account> for AccountSharedData {
    fn from(other: Account) -> Self {
        Self {
            lamports: other.lamports,
            data: Arc::new(other.data),
            owner: other.owner,
            executable: other.executable,
            rent_epoch: other.rent_epoch,
        }
    }
}

pub trait WritableAccount: ReadableAccount {
    fn set_lamports(&mut self, lamports: u64);
    fn checked_add_lamports(&mut self, lamports: u64) -> Result<(), LamportsError> {
        self.set_lamports(
            self.lamports()
                .checked_add(lamports)
                .ok_or(LamportsError::ArithmeticOverflow)?,
        );
        Ok(())
    }
    fn checked_sub_lamports(&mut self, lamports: u64) -> Result<(), LamportsError> {
        self.set_lamports(
            self.lamports()
                .checked_sub(lamports)
                .ok_or(LamportsError::ArithmeticUnderflow)?,
        );
        Ok(())
    }
    fn saturating_add_lamports(&mut self, lamports: u64) {
        self.set_lamports(self.lamports().saturating_add(lamports))
    }
    fn saturating_sub_lamports(&mut self, lamports: u64) {
        self.set_lamports(self.lamports().saturating_sub(lamports))
    }
    fn data_as_mut_slice(&mut self) -> &mut [u8];
    fn set_owner(&mut self, owner: Pubkey);
    fn copy_into_owner_from_slice(&mut self, source: &[u8]);
    fn set_executable(&mut self, executable: bool);
    fn set_rent_epoch(&mut self, epoch: Epoch);
    fn create(
        lamports: u64,
        data: Vec<u8>,
        owner: Pubkey,
        executable: bool,
        rent_epoch: Epoch,
    ) -> Self;
}

pub trait ReadableAccount: Sized {
    fn lamports(&self) -> u64;
    fn data(&self) -> &[u8];
    fn owner(&self) -> &Pubkey;
    fn executable(&self) -> bool;
    fn rent_epoch(&self) -> Epoch;
    fn to_account_shared_data(&self) -> AccountSharedData {
        AccountSharedData::create(
            self.lamports(),
            self.data().to_vec(),
            *self.owner(),
            self.executable(),
            self.rent_epoch(),
        )
    }
}

impl ReadableAccount for Account {
    fn lamports(&self) -> u64 {
        self.lamports
    }
    fn data(&self) -> &[u8] {
        &self.data
    }
    fn owner(&self) -> &Pubkey {
        &self.owner
    }
    fn executable(&self) -> bool {
        self.executable
    }
    fn rent_epoch(&self) -> Epoch {
        self.rent_epoch
    }
}

impl WritableAccount for Account {
    fn set_lamports(&mut self, lamports: u64) {
        self.lamports = lamports;
    }
    fn data_as_mut_slice(&mut self) -> &mut [u8] {
        &mut self.data
    }
    fn set_owner(&mut self, owner: Pubkey) {
        self.owner = owner;
    }
    fn copy_into_owner_from_slice(&mut self, source: &[u8]) {
        self.owner.as_mut().copy_from_slice(source);
    }
    fn set_executable(&mut self, executable: bool) {
        self.executable = executable;
    }
    fn set_rent_epoch(&mut self, epoch: Epoch) {
        self.rent_epoch = epoch;
    }
    fn create(
        lamports: u64,
        data: Vec<u8>,
        owner: Pubkey,
        executable: bool,
        rent_epoch: Epoch,
    ) -> Self {
        Account {
            lamports,
            data,
            owner,
            executable,
            rent_epoch,
        }
    }
}

impl WritableAccount for AccountSharedData {
    fn set_lamports(&mut self, lamports: u64) {
        self.lamports = lamports;
    }
    fn data_as_mut_slice(&mut self) -> &mut [u8] {
        &mut self.data_mut()[..]
    }
    fn set_owner(&mut self, owner: Pubkey) {
        self.owner = owner;
    }
    fn copy_into_owner_from_slice(&mut self, source: &[u8]) {
        self.owner.as_mut().copy_from_slice(source);
    }
    fn set_executable(&mut self, executable: bool) {
        self.executable = executable;
    }
    fn set_rent_epoch(&mut self, epoch: Epoch) {
        self.rent_epoch = epoch;
    }
    fn create(
        lamports: u64,
        data: Vec<u8>,
        owner: Pubkey,
        executable: bool,
        rent_epoch: Epoch,
    ) -> Self {
        AccountSharedData {
            lamports,
            data: Arc::new(data),
            owner,
            executable,
            rent_epoch,
        }
    }
}

impl ReadableAccount for AccountSharedData {
    fn lamports(&self) -> u64 {
        self.lamports
    }
    fn data(&self) -> &[u8] {
        &self.data
    }
    fn owner(&self) -> &Pubkey {
        &self.owner
    }
    fn executable(&self) -> bool {
        self.executable
    }
    fn rent_epoch(&self) -> Epoch {
        self.rent_epoch
    }
    fn to_account_shared_data(&self) -> AccountSharedData {
        // avoid data copy here
        self.clone()
    }
}

impl ReadableAccount for Ref<'_, AccountSharedData> {
    fn lamports(&self) -> u64 {
        self.lamports
    }
    fn data(&self) -> &[u8] {
        &self.data
    }
    fn owner(&self) -> &Pubkey {
        &self.owner
    }
    fn executable(&self) -> bool {
        self.executable
    }
    fn rent_epoch(&self) -> Epoch {
        self.rent_epoch
    }
    fn to_account_shared_data(&self) -> AccountSharedData {
        AccountSharedData {
            lamports: self.lamports(),
            // avoid data copy here
            data: Arc::clone(&self.data),
            owner: *self.owner(),
            executable: self.executable(),
            rent_epoch: self.rent_epoch(),
        }
    }
}

impl ReadableAccount for Ref<'_, Account> {
    fn lamports(&self) -> u64 {
        self.lamports
    }
    fn data(&self) -> &[u8] {
        &self.data
    }
    fn owner(&self) -> &Pubkey {
        &self.owner
    }
    fn executable(&self) -> bool {
        self.executable
    }
    fn rent_epoch(&self) -> Epoch {
        self.rent_epoch
    }
}

fn debug_fmt<T: ReadableAccount>(item: &T, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    let mut f = f.debug_struct("Account");

    f.field("lamports", &item.lamports())
        .field("data.len", &item.data().len())
        .field("owner", &item.owner())
        .field("executable", &item.executable())
        .field("rent_epoch", &item.rent_epoch());
    debug_account_data(item.data(), &mut f);

    f.finish()
}

impl fmt::Debug for Account {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        debug_fmt(self, f)
    }
}

impl fmt::Debug for AccountSharedData {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        debug_fmt(self, f)
    }
}

fn shared_new<T: WritableAccount>(lamports: u64, space: usize, owner: &Pubkey) -> T {
    T::create(
        lamports,
        vec![0u8; space],
        *owner,
        bool::default(),
        Epoch::default(),
    )
}

fn shared_new_rent_epoch<T: WritableAccount>(
    lamports: u64,
    space: usize,
    owner: &Pubkey,
    rent_epoch: Epoch,
) -> T {
    T::create(
        lamports,
        vec![0u8; space],
        *owner,
        bool::default(),
        rent_epoch,
    )
}

fn shared_new_ref<T: WritableAccount>(
    lamports: u64,
    space: usize,
    owner: &Pubkey,
) -> Rc<RefCell<T>> {
    Rc::new(RefCell::new(shared_new::<T>(lamports, space, owner)))
}

fn shared_new_data<T: serde::Serialize, U: WritableAccount>(
    lamports: u64,
    state: &T,
    owner: &Pubkey,
) -> Result<U, bincode::Error> {
    let data = bincode::serialize(state)?;
    Ok(U::create(
        lamports,
        data,
        *owner,
        bool::default(),
        Epoch::default(),
    ))
}
fn shared_new_ref_data<T: serde::Serialize, U: WritableAccount>(
    lamports: u64,
    state: &T,
    owner: &Pubkey,
) -> Result<RefCell<U>, bincode::Error> {
    Ok(RefCell::new(shared_new_data::<T, U>(
        lamports, state, owner,
    )?))
}

fn shared_new_data_with_space<T: serde::Serialize, U: WritableAccount>(
    lamports: u64,
    state: &T,
    space: usize,
    owner: &Pubkey,
) -> Result<U, bincode::Error> {
    let mut account = shared_new::<U>(lamports, space, owner);

    shared_serialize_data(&mut account, state)?;

    Ok(account)
}
fn shared_new_ref_data_with_space<T: serde::Serialize, U: WritableAccount>(
    lamports: u64,
    state: &T,
    space: usize,
    owner: &Pubkey,
) -> Result<RefCell<U>, bincode::Error> {
    Ok(RefCell::new(shared_new_data_with_space::<T, U>(
        lamports, state, space, owner,
    )?))
}

fn shared_deserialize_data<T: serde::de::DeserializeOwned, U: ReadableAccount>(
    account: &U,
) -> Result<T, bincode::Error> {
    bincode::deserialize(account.data())
}

fn shared_serialize_data<T: serde::Serialize, U: WritableAccount>(
    account: &mut U,
    state: &T,
) -> Result<(), bincode::Error> {
    if bincode::serialized_size(state)? > account.data().len() as u64 {
        return Err(Box::new(bincode::ErrorKind::SizeLimit));
    }
    bincode::serialize_into(account.data_as_mut_slice(), state)
}

impl Account {
    pub fn new(lamports: u64, space: usize, owner: &Pubkey) -> Self {
        shared_new(lamports, space, owner)
    }
    pub fn new_ref(lamports: u64, space: usize, owner: &Pubkey) -> Rc<RefCell<Self>> {
        shared_new_ref(lamports, space, owner)
    }
    pub fn new_data<T: serde::Serialize>(
        lamports: u64,
        state: &T,
        owner: &Pubkey,
    ) -> Result<Self, bincode::Error> {
        shared_new_data(lamports, state, owner)
    }
    pub fn new_ref_data<T: serde::Serialize>(
        lamports: u64,
        state: &T,
        owner: &Pubkey,
    ) -> Result<RefCell<Self>, bincode::Error> {
        shared_new_ref_data(lamports, state, owner)
    }
    pub fn new_data_with_space<T: serde::Serialize>(
        lamports: u64,
        state: &T,
        space: usize,
        owner: &Pubkey,
    ) -> Result<Self, bincode::Error> {
        shared_new_data_with_space(lamports, state, space, owner)
    }
    pub fn new_ref_data_with_space<T: serde::Serialize>(
        lamports: u64,
        state: &T,
        space: usize,
        owner: &Pubkey,
    ) -> Result<RefCell<Self>, bincode::Error> {
        shared_new_ref_data_with_space(lamports, state, space, owner)
    }
    pub fn new_rent_epoch(lamports: u64, space: usize, owner: &Pubkey, rent_epoch: Epoch) -> Self {
        shared_new_rent_epoch(lamports, space, owner, rent_epoch)
    }
    pub fn deserialize_data<T: serde::de::DeserializeOwned>(&self) -> Result<T, bincode::Error> {
        shared_deserialize_data(self)
    }
    pub fn serialize_data<T: serde::Serialize>(&mut self, state: &T) -> Result<(), bincode::Error> {
        shared_serialize_data(self, state)
    }
}

impl AccountSharedData {
    pub fn is_shared(&self) -> bool {
        Arc::strong_count(&self.data) > 1
    }

    pub fn reserve(&mut self, additional: usize) {
        self.data_mut().reserve(additional)
    }

    pub fn capacity(&self) -> usize {
        self.data.capacity()
    }

    fn data_mut(&mut self) -> &mut Vec<u8> {
        Arc::make_mut(&mut self.data)
    }

    pub fn resize(&mut self, new_len: usize, value: u8) {
        self.data_mut().resize(new_len, value)
    }

    pub fn extend_from_slice(&mut self, data: &[u8]) {
        self.data_mut().extend_from_slice(data)
    }

    pub fn set_data_from_slice(&mut self, new_data: &[u8]) {
        // If the buffer isn't shared, we're going to memcpy in place.
        let Some(data) = Arc::get_mut(&mut self.data) else {
            // If the buffer is shared, the cheapest thing to do is to clone the
            // incoming slice and replace the buffer.
            return self.set_data(new_data.to_vec());
        };

        let new_len = new_data.len();

        // Reserve additional capacity if needed. Here we make the assumption
        // that growing the current buffer is cheaper than doing a whole new
        // allocation to make `new_data` owned.
        //
        // This assumption holds true during CPI, especially when the account
        // size doesn't change but the account is only changed in place. And
        // it's also true when the account is grown by a small margin (the
        // realloc limit is quite low), in which case the allocator can just
        // update the allocation metadata without moving.
        //
        // Shrinking and copying in place is always faster than making
        // `new_data` owned, since shrinking boils down to updating the Vec's
        // length.

        data.reserve(new_len.saturating_sub(data.len()));

        // Safety:
        // We just reserved enough capacity. We set data::len to 0 to avoid
        // possible UB on panic (dropping uninitialized elements), do the copy,
        // finally set the new length once everything is initialized.
        #[allow(clippy::uninit_vec)]
        // this is a false positive, the lint doesn't currently special case set_len(0)
        unsafe {
            data.set_len(0);
            ptr::copy_nonoverlapping(new_data.as_ptr(), data.as_mut_ptr(), new_len);
            data.set_len(new_len);
        };
    }

    fn set_data(&mut self, data: Vec<u8>) {
        self.data = Arc::new(data);
    }

    pub fn spare_data_capacity_mut(&mut self) -> &mut [MaybeUninit<u8>] {
        self.data_mut().spare_capacity_mut()
    }

    pub fn new(lamports: u64, space: usize, owner: &Pubkey) -> Self {
        shared_new(lamports, space, owner)
    }
    pub fn new_ref(lamports: u64, space: usize, owner: &Pubkey) -> Rc<RefCell<Self>> {
        shared_new_ref(lamports, space, owner)
    }
    pub fn new_data<T: serde::Serialize>(
        lamports: u64,
        state: &T,
        owner: &Pubkey,
    ) -> Result<Self, bincode::Error> {
        shared_new_data(lamports, state, owner)
    }
    pub fn new_ref_data<T: serde::Serialize>(
        lamports: u64,
        state: &T,
        owner: &Pubkey,
    ) -> Result<RefCell<Self>, bincode::Error> {
        shared_new_ref_data(lamports, state, owner)
    }
    pub fn new_data_with_space<T: serde::Serialize>(
        lamports: u64,
        state: &T,
        space: usize,
        owner: &Pubkey,
    ) -> Result<Self, bincode::Error> {
        shared_new_data_with_space(lamports, state, space, owner)
    }
    pub fn new_ref_data_with_space<T: serde::Serialize>(
        lamports: u64,
        state: &T,
        space: usize,
        owner: &Pubkey,
    ) -> Result<RefCell<Self>, bincode::Error> {
        shared_new_ref_data_with_space(lamports, state, space, owner)
    }
    pub fn new_rent_epoch(lamports: u64, space: usize, owner: &Pubkey, rent_epoch: Epoch) -> Self {
        shared_new_rent_epoch(lamports, space, owner, rent_epoch)
    }
    pub fn deserialize_data<T: serde::de::DeserializeOwned>(&self) -> Result<T, bincode::Error> {
        shared_deserialize_data(self)
    }
    pub fn serialize_data<T: serde::Serialize>(&mut self, state: &T) -> Result<(), bincode::Error> {
        shared_serialize_data(self, state)
    }
}
