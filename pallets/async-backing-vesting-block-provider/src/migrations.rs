use super::*;

use frame_support::{pallet_prelude::*, weights::Weight};

pub(crate) fn on_runtime_upgrade<T: Config>() -> Weight {
	upgrade::MigrateToV0::<T>::on_runtime_upgrade()
}

mod upgrade {
	use super::*;

	pub struct MigrateToV0<T>(sp_std::marker::PhantomData<T>);

	// This migration will trigger for krest runtime, but not peaq runtime
	// since peaq will have already been migrated to this storage version with pallet version 0.1.0
	impl<T: Config> MigrateToV0<T> {
		pub fn on_runtime_upgrade() -> Weight {
			let mut weight_writes = 0;
			let mut weight_reads = 0;
			let calculated_weight: Weight = Weight::default();

			// get storage versions
			let onchain_storage_version = Pallet::<T>::on_chain_storage_version();
			weight_reads += 1;
			let current = Pallet::<T>::current_storage_version();

			if onchain_storage_version < current {
				AsyncBackingAt::<T>::put(frame_system::Pallet::<T>::current_block_number());
				weight_reads += 1;
				weight_writes += 1;

				// Update storage version
				STORAGE_VERSION.put::<Pallet<T>>();
				weight_writes += 1;

				log::info!(
					"async backing vesting block provider storage migration completed from version {:?} to version {:?}", onchain_storage_version, current
				);
			}
			calculated_weight
				.saturating_add(T::DbWeight::get().reads_writes(weight_reads, weight_writes))
		}
	}
}
