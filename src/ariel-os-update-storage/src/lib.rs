#![cfg_attr(not(test), no_std)]
// #![deny(missing_docs)]

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum StorageBackend {
    Nvm,
    // Ram,
    // Filesystem,
}

// NOTE: not to be confused with a SUIT Slot (which refers to a range within a SUIT Component).
#[derive(Debug, Copy, Clone)]
pub struct UpdateSlot<'id> {
    storage_backend: StorageBackend,
    // Opaque.
    // NOTE: This can be constructed from a SUIT Component Identifier and a SUIT Slot by concatenating them.
    id: &'id [u8],
}

impl<'id> UpdateSlot<'id> {
    #[must_use]
    pub fn new(storage_backend: StorageBackend, id: &'id [u8]) -> Self {
        Self {
            storage_backend,
            id,
        }
    }

    pub fn storage_backend(&self) -> StorageBackend {
        self.storage_backend
    }

    pub fn id(&self) -> &'id [u8] {
        self.id
    }
}

/// Reads the given update slot, starting from the offset (within the slot),
/// for the given length.
pub async fn read(update_slot: UpdateSlot<'_>, offset: u32, buf: &mut [u8]) -> Result<(), Error> {
    todo!();
}

/// Writes the given payload to an update slot, at the given offset
/// within that slot.
// NOTE: `bytes` is a subslice of a SUIT payload.
pub async fn write(bytes: &[u8], update_slot: UpdateSlot<'_>, offset: u32) -> Result<(), Error> {
    todo!();
}

/// Returns the length of the payload written since the last reboot or the last update.
pub fn len(update_slot: UpdateSlot<'_>) -> Result<u32, Error> {
    todo!();
}

/// Returns the capacity of the given update slot.
pub fn capacity(update_slot: UpdateSlot<'_>) -> Result<u32, Error> {
    todo!();
}

#[derive(Debug)]
pub enum Error {
    /// Write failed because of a storage error.
    Storage,
    /// Invalid update slot (includes invalid backend).
    InvalidSlot,
    /// The offset is larger than the slot's size.
    OffsetOutOfBounds,
    /// The byte slice given is too large for the remaining space in the slot (starting at the offset).
    InvalidByteSliceLength,
}

impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            _ => todo!(),
        }
    }
}

impl core::error::Error for Error {}
