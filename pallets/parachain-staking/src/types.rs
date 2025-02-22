// KILT Blockchain – https://botlabs.org
// Copyright (C) 2019-2022 BOTLabs GmbH

// The KILT Blockchain is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.

// The KILT Blockchain is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.

// You should have received a copy of the GNU General Public License
// along with this program.  If not, see <https://www.gnu.org/licenses/>.

// If you feel like getting in touch with us, you can do so at info@botlabs.org

use frame_support::traits::{Currency, Get};
use parity_scale_codec::{Decode, Encode, MaxEncodedLen};
use scale_info::TypeInfo;
use sp_runtime::{
	traits::{AtLeast32BitUnsigned, Saturating, Zero},
	Permill, RuntimeDebug,
};
use sp_staking::SessionIndex;
use sp_std::{
	cmp::Ordering,
	convert::TryInto,
	fmt::Debug,
	ops::{Add, Sub},
	vec,
	vec::Vec,
};

use crate::{set::OrderedSet, Config};

/// A struct represented an amount of staked funds.
///
/// The stake has a destination account (to which the stake is directed) and an
/// amount of funds staked.
#[derive(Default, Clone, Encode, Decode, RuntimeDebug, PartialEq, Eq, TypeInfo, MaxEncodedLen)]
#[codec(mel_bound(AccountId: MaxEncodedLen, Balance: MaxEncodedLen))]
pub struct Stake<AccountId, Balance>
where
	AccountId: Eq + Ord,
	Balance: Eq + Ord,
{
	/// The account that is backed by the stake.
	pub owner: AccountId,

	/// The amount of backing the `owner` received.
	pub amount: Balance,
}

impl<A, B> From<A> for Stake<A, B>
where
	A: Eq + Ord,
	B: Default + Eq + Ord,
{
	fn from(owner: A) -> Self {
		Stake { owner, amount: B::default() }
	}
}

impl<AccountId: Ord, Balance: PartialEq + Ord> PartialOrd for Stake<AccountId, Balance> {
	fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
		Some(self.cmp(other))
	}
}

// We order by stake and only return an equal order, if both account ids match.
// This prevents the same account ids to be in the same OrderedSet. Otherwise,
// it is ordered from greatest to lowest stake (primary) and from first joined
// to last joined (primary).
impl<AccountId: Ord, Balance: PartialEq + Ord> Ord for Stake<AccountId, Balance> {
	fn cmp(&self, other: &Self) -> Ordering {
		match (self.owner.cmp(&other.owner), self.amount.cmp(&other.amount)) {
			// enforce unique account ids
			(Ordering::Equal, _) => Ordering::Equal,
			// prioritize existing members if stakes match
			(_, Ordering::Equal) => Ordering::Greater,
			// order by stake
			(_, ord) => ord,
		}
	}
}

pub type Reward<AccountId, Balance> = Stake<AccountId, Balance>;

/// The activity status of the collator.
#[derive(
	Copy, Clone, Default, PartialEq, Eq, Encode, Decode, RuntimeDebug, TypeInfo, MaxEncodedLen,
)]
pub enum CandidateStatus {
	/// Committed to be online and producing valid blocks (not equivocating)
	#[default]
	Active,
	/// Staked until the inner round
	Leaving(SessionIndex),
}

#[derive(Encode, Decode, Clone, RuntimeDebug, PartialEq, Eq, TypeInfo, MaxEncodedLen)]
#[scale_info(skip_type_params(MaxDelegatorsPerCandidate))]
#[codec(mel_bound(AccountId: MaxEncodedLen, Balance: MaxEncodedLen))]
pub struct OldCandidate<AccountId, Balance, MaxDelegatorsPerCandidate>
where
	AccountId: Eq + Ord + Debug,
	Balance: Eq + Ord + Debug,
	MaxDelegatorsPerCandidate: Get<u32> + Debug + PartialEq,
{
	pub id: AccountId,
	pub stake: Balance,
	pub delegators: OrderedSet<Stake<AccountId, Balance>, MaxDelegatorsPerCandidate>,
	pub total: Balance,
	pub status: CandidateStatus,
}

#[derive(Encode, Decode, Clone, RuntimeDebug, PartialEq, Eq, TypeInfo, MaxEncodedLen)]
#[scale_info(skip_type_params(MaxDelegatorsPerCandidate))]
#[codec(mel_bound(AccountId: MaxEncodedLen, Balance: MaxEncodedLen))]
/// Global collator state with commission fee, staked funds, and delegations
pub struct Candidate<AccountId, Balance, MaxDelegatorsPerCandidate>
where
	AccountId: Eq + Ord + Debug,
	Balance: Eq + Ord + Debug,
	MaxDelegatorsPerCandidate: Get<u32> + Debug + PartialEq,
{
	/// Account id of the candidate.
	pub id: AccountId,

	/// The stake that the candidate put down.
	pub stake: Balance,

	/// The delegators that back the candidate.
	pub delegators: OrderedSet<Stake<AccountId, Balance>, MaxDelegatorsPerCandidate>,

	/// The total backing a collator has.
	///
	/// Should equal the sum of all delegators stake adding collators stake
	pub total: Balance,

	/// The current status of the candidate. Indicates whether a candidate is
	/// active or leaving the candidate pool
	pub status: CandidateStatus,

	/// Commission of the collator
	pub commission: Permill,
}

impl<A, B, S> Candidate<A, B, S>
where
	A: Ord + Clone + Debug,
	B: AtLeast32BitUnsigned + Ord + Copy + Saturating + Debug + Zero,
	S: Get<u32> + Debug + PartialEq,
{
	pub fn new(id: A, stake: B) -> Self {
		let total = stake;
		Candidate {
			id,
			stake,
			delegators: OrderedSet::new(),
			total,
			status: CandidateStatus::default(), // default active
			commission: Permill::zero(),
		}
	}

	pub fn is_active(&self) -> bool {
		self.status == CandidateStatus::Active
	}

	pub fn is_leaving(&self) -> bool {
		matches!(self.status, CandidateStatus::Leaving(_))
	}

	pub fn can_exit(&self, when: u32) -> bool {
		matches!(self.status, CandidateStatus::Leaving(at) if at <= when )
	}

	pub fn revert_leaving(&mut self) {
		self.status = CandidateStatus::Active;
	}

	pub fn stake_more(&mut self, more: B) {
		self.stake = self.stake.saturating_add(more);
		self.total = self.total.saturating_add(more);
	}

	// Returns None if underflow or less == self.stake (in which case collator
	// should leave).
	pub fn stake_less(&mut self, less: B) -> Option<B> {
		if self.stake > less {
			self.stake = self.stake.saturating_sub(less);
			self.total = self.total.saturating_sub(less);
			Some(self.stake)
		} else {
			None
		}
	}

	pub fn inc_delegator(&mut self, delegator: A, more: B) {
		if let Ok(i) = self
			.delegators
			.linear_search(&Stake::<A, B> { owner: delegator, amount: B::zero() })
		{
			self.delegators.mutate(|vec| vec[i].amount = vec[i].amount.saturating_add(more));
			self.total = self.total.saturating_add(more);
			self.delegators.sort_greatest_to_lowest()
		}
	}

	pub fn dec_delegator(&mut self, delegator: A, less: B) {
		if let Ok(i) = self
			.delegators
			.linear_search(&Stake::<A, B> { owner: delegator, amount: B::zero() })
		{
			self.delegators.mutate(|vec| vec[i].amount = vec[i].amount.saturating_sub(less));
			self.total = self.total.saturating_sub(less);
			self.delegators.sort_greatest_to_lowest()
		}
	}

	pub fn leave_candidates(&mut self, round: SessionIndex) {
		self.status = CandidateStatus::Leaving(round);
	}

	pub fn set_commission(&mut self, commission: Permill) {
		self.commission = commission;
	}
}

#[derive(Encode, Decode, RuntimeDebug, PartialEq, Eq, TypeInfo, MaxEncodedLen)]
#[scale_info(skip_type_params(MaxCollatorsPerDelegator))]
#[codec(mel_bound(AccountId: MaxEncodedLen, Balance: MaxEncodedLen))]
pub struct Delegator<AccountId: Eq + Ord, Balance: Eq + Ord, MaxCollatorsPerDelegator: Get<u32>> {
	pub delegations: OrderedSet<Stake<AccountId, Balance>, MaxCollatorsPerDelegator>,
	pub total: Balance,
}

impl<AccountId, Balance, MaxCollatorsPerDelegator>
	Delegator<AccountId, Balance, MaxCollatorsPerDelegator>
where
	AccountId: Eq + Ord + Clone + Debug,
	Balance: Copy + Add<Output = Balance> + Saturating + PartialOrd + Eq + Ord + Debug + Zero,
	MaxCollatorsPerDelegator: Get<u32> + Debug + PartialEq,
{
	pub fn try_new(
		collator: AccountId,
		amount: Balance,
	) -> Result<Self, Vec<Stake<AccountId, Balance>>> {
		Ok(Delegator {
			delegations: OrderedSet::from(
				vec![Stake { owner: collator, amount }].try_into()?, //.unwrap(),
			),
			total: amount,
		})
	}

	/// Adds a new delegation.
	///
	/// If already delegating to the same account, this call returns false and
	/// doesn't insert the new delegation.
	pub fn add_delegation(&mut self, stake: Stake<AccountId, Balance>) -> Result<bool, usize> {
		let amt = stake.amount;
		if self.delegations.try_insert(stake)? {
			self.total = self.total.saturating_add(amt);
			Ok(true)
		} else {
			Ok(false)
		}
	}

	/// Returns Some(remaining stake for delegator) if the delegation for the
	/// collator exists. Returns `None` otherwise.
	pub fn rm_delegation(&mut self, collator: &AccountId) -> Option<Balance> {
		let amt = self.delegations.remove(&Stake::<AccountId, Balance> {
			owner: collator.clone(),
			// amount is irrelevant for removal
			amount: Balance::zero(),
		});

		if let Some(Stake::<AccountId, Balance> { amount: balance, .. }) = amt {
			self.total = self.total.saturating_sub(balance);
			Some(self.total)
		} else {
			None
		}
	}

	/// Returns None if delegation was not found.
	pub fn inc_delegation(&mut self, collator: AccountId, more: Balance) -> Option<Balance> {
		if let Ok(i) = self.delegations.linear_search(&Stake::<AccountId, Balance> {
			owner: collator,
			amount: Balance::zero(),
		}) {
			let amount = self.delegations[i].amount.saturating_add(more);

			self.delegations
				.mutate(|vec| vec[i].amount = vec[i].amount.saturating_add(more));
			self.total = self.total.saturating_add(more);
			self.delegations.sort_greatest_to_lowest();
			Some(amount)
		} else {
			None
		}
	}

	/// Returns Some(Some(balance)) if successful, None if delegation was not
	/// found and Some(None) if delegated stake would underflow.
	pub fn dec_delegation(
		&mut self,
		collator: AccountId,
		less: Balance,
	) -> Option<Option<Balance>> {
		if let Ok(i) = self.delegations.linear_search(&Stake::<AccountId, Balance> {
			owner: collator,
			amount: Balance::zero(),
		}) {
			if self.delegations[i].amount > less {
				let amount = self.delegations[i].amount.saturating_sub(less);

				self.delegations
					.mutate(|vec| vec[i].amount = vec[i].amount.saturating_sub(less));
				self.total = self.total.saturating_sub(less);
				self.delegations.sort_greatest_to_lowest();
				Some(Some(amount))
			} else {
				// underflow error; should rm entire delegation
				Some(None)
			}
		} else {
			None
		}
	}
}

/// The current round index and transition information.
#[derive(Copy, Clone, PartialEq, Eq, Encode, Decode, RuntimeDebug, TypeInfo, MaxEncodedLen)]
pub struct RoundInfo<BlockNumber> {
	/// Current round index.
	pub current: SessionIndex,
	/// The first block of the current round.
	pub first: BlockNumber,
	/// The length of the current round in blocks.
	pub length: BlockNumber,
}

impl<B> RoundInfo<B>
where
	B: Copy + Saturating + From<u32> + PartialOrd,
{
	pub fn new(current: SessionIndex, first: B, length: B) -> RoundInfo<B> {
		RoundInfo { current, first, length }
	}

	/// Checks if the round should be updated.
	///
	/// The round should update if `self.length` or more blocks where produced
	/// after `self.first`.
	pub fn should_update(&self, now: B) -> bool {
		let l = now.saturating_sub(self.first);
		l >= self.length
	}

	/// Starts a new round.
	pub fn update(&mut self, now: B) {
		self.current = self.current.saturating_add(1u32);
		self.first = now;
	}
}

impl<B> Default for RoundInfo<B>
where
	B: Copy + Saturating + Add<Output = B> + Sub<Output = B> + From<u32> + PartialOrd,
{
	fn default() -> RoundInfo<B> {
		RoundInfo::new(0u32, 0u32.into(), 20.into())
	}
}

/// The total stake of the pallet.
///
/// The stake includes both collators' and delegators' staked funds.
#[derive(Default, Clone, Encode, Decode, RuntimeDebug, PartialEq, Eq, TypeInfo, MaxEncodedLen)]
pub struct TotalStake<Balance: Default> {
	pub collators: Balance,
	pub delegators: Balance,
}

/// The number of delegations a delegator has done within the last session in
/// which they delegated.
#[derive(Default, Clone, Encode, Decode, RuntimeDebug, PartialEq, Eq, TypeInfo, MaxEncodedLen)]
pub struct DelegationCounter {
	/// The index of the last delegation.
	pub round: SessionIndex,
	/// The number of delegations made within round.
	pub counter: u32,
}

/// Internal type which is only used when a delegator is replaced by another
/// one to delay the storage entry removal until failure cannot happen anymore.
pub(crate) struct ReplacedDelegator<T: Config> {
	pub who: AccountIdOf<T>,
	pub state: Option<Delegator<AccountIdOf<T>, BalanceOf<T>, T::MaxCollatorsPerDelegator>>,
}

pub type AccountIdOf<T> = <T as frame_system::Config>::AccountId;
pub type BalanceOf<T> = <<T as Config>::Currency as Currency<AccountIdOf<T>>>::Balance;
pub type CandidateOf<T, S> = Candidate<AccountIdOf<T>, BalanceOf<T>, S>;
pub type MaxDelegatorsPerCollator<T> = <T as Config>::MaxDelegatorsPerCollator;
pub type StakeOf<T> = Stake<AccountIdOf<T>, BalanceOf<T>>;

#[derive(Default, Clone, Encode, Decode, RuntimeDebug, PartialEq, Eq, TypeInfo, MaxEncodedLen)]
/// Info needed to make delayed payments to stakers after round end
pub struct DelayedPayoutInfoT<SessionIndex, Balance: Default> {
	/// The round index for which payouts should be made
	pub round: SessionIndex,
	/// total stake in the round
	pub total_stake: Balance,
	/// total issuance for round
	pub total_issuance: Balance,
}
